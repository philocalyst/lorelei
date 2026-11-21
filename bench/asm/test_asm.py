#!/usr/bin/env python3
"""Fast fail-closed tests for asm.py (no cargo, no build): python3 bench/asm/test_asm.py"""
import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("asm", Path(__file__).resolve().parent / "asm.py")
asm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(asm)

CTX = "rustc rustc 1.0\nprofile release\ncodegen none\nrustflags none\nhost arm64 macOS 1\n"


def emitted(root, name, counts, ctx=CTX, rows=None):
    d = Path(root) / name
    d.mkdir(parents=True)
    (d / "MANIFEST.txt").write_text(ctx)
    if rows is None:
        rows = "".join(f"{e}\tp\tsym_{e}\t{n}\t0\t0\t0\t0\t0\n" for e, n in counts.items())
    (d / "stats.tsv").write_text("# header\n" + rows)
    return d


def run(fn, *a):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        try:
            rc = fn(*a)
        except SystemExit as e:
            rc = e.code if isinstance(e.code, int) else 99
    return rc, out.getvalue()


class Diff(unittest.TestCase):
    def setUp(self):
        self.t = tempfile.TemporaryDirectory()
        self.names = [e["name"] for e in asm.entries()]
        self.base = {n: 100 for n in self.names}

    def tearDown(self):
        self.t.cleanup()

    def dirs(self, head, **kw):
        return (emitted(self.t.name, "b", self.base), emitted(self.t.name, "h", head, **kw))

    def test_same_passes(self):
        b, h = self.dirs(dict(self.base))
        self.assertEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_growth_fails(self):
        b, h = self.dirs({**self.base, self.names[0]: 130})
        rc, out = run(asm.diff, b, h, 0.02)
        self.assertEqual(rc, 1)
        self.assertIn("REGRESSED", out)

    def test_small_growth_passes(self):
        b, h = self.dirs({**self.base, self.names[0]: 110})   # +10 <= 16 instructions
        self.assertEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_profile_mismatch_fails(self):
        b, h = self.dirs(dict(self.base), ctx=CTX.replace("release", "dev"))
        rc, out = run(asm.diff, b, h, 0.02)
        self.assertEqual(rc, 2)
        self.assertIn("profile", out)

    def test_toolchain_mismatch_fails(self):
        b, h = self.dirs(dict(self.base), ctx=CTX.replace("1.0", "2.0"))
        self.assertEqual(run(asm.diff, b, h, 0.02)[0], 2)

    def test_missing_context_file_fails(self):
        b, h = self.dirs(dict(self.base))
        (h / "MANIFEST.txt").unlink()
        self.assertNotEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_empty_stats_fails(self):
        b, h = self.dirs(dict(self.base), rows="")
        self.assertNotEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_malformed_row_fails(self):
        b, h = self.dirs(dict(self.base), rows="turn_settle\tp\ts\tNaN\t0\t0\t0\t0\t0\n")
        self.assertNotEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_short_row_fails(self):
        b, h = self.dirs(dict(self.base), rows="turn_settle\tp\ts\t5\n")
        self.assertNotEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_partial_emit_fails(self):
        head = dict(self.base)
        head.pop(self.names[0])
        b, h = self.dirs(head)
        self.assertNotEqual(run(asm.diff, b, h, 0.02)[0], 0)

    def test_zero_base_fails(self):
        self.base[self.names[0]] = 0
        b, h = self.dirs(dict(self.base))
        self.assertEqual(run(asm.diff, b, h, 0.02)[0], 1)

    def test_not_a_directory_fails(self):
        self.assertNotEqual(run(asm.diff, self.t.name + "/nope", self.t.name + "/nope2", 0.02)[0], 0)

    def test_negative_threshold_fails(self):
        b, h = self.dirs(dict(self.base))
        self.assertNotEqual(run(asm.diff, b, h, -1.0)[0], 0)


class Env(unittest.TestCase):
    def test_ambient_flags_frozen(self):
        amb = {"PATH": "/bin", "RUSTFLAGS": "-C target-cpu=native", "CARGO_ENCODED_RUSTFLAGS": "-Cx",
               "CARGO_BUILD_RUSTFLAGS": "-Cy", "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS": "-Cz", "HOME": "/h"}
        env, ign = asm.frozen_env(None, amb)
        self.assertEqual(sorted(env), ["HOME", "PATH"])
        self.assertEqual(len(ign), 4)
        self.assertNotIn("native", " ".join(ign))      # digests only
        env, _ = asm.frozen_env("-C opt-level=2", amb)
        self.assertEqual(env["RUSTFLAGS"], "-C opt-level=2")
        self.assertNotIn("CARGO_ENCODED_RUSTFLAGS", env)

    def test_clean_env_untouched(self):
        env, ign = asm.frozen_env(None, {"PATH": "/bin"})
        self.assertEqual((env, ign), ({"PATH": "/bin"}, []))


class Manifest(unittest.TestCase):
    def test_bad_manifest_rejected(self):
        old = asm.MANIFEST
        try:
            with tempfile.TemporaryDirectory() as t:
                for text in ("a\tp\tbin\tx\n", "a\tp\twat\tx\ty\n", "a\tp\tbin\tx\ty\na\tp\tbin\tx\ty\n", "# only\n"):
                    f = Path(t) / "m.tsv"
                    f.write_text(text)
                    asm.MANIFEST = f
                    with self.assertRaises(SystemExit):
                        asm.entries()
        finally:
            asm.MANIFEST = old

    def test_real_manifest_parses(self):
        self.assertGreater(len(asm.entries()), 10)


class Graphs(unittest.TestCase):
    def test_calls_empty_dir_fails(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertNotEqual(run(asm.graph_calls, Path(t) / "o.svg", t, 2)[0], 0)

    def test_calls_no_edges_fails(self):
        with tempfile.TemporaryDirectory() as t:
            (Path(t) / "fn").mkdir()
            (Path(t) / "fn" / "x.s").write_text("x:\n\tadd x0, x0, x1\n\tret\n")
            self.assertNotEqual(run(asm.graph_calls, Path(t) / "o.svg", t, 2)[0], 0)


class Stats(unittest.TestCase):
    def test_counts(self):
        body = ["\tadd x0, x0, x1", "\tbl <core::panicking::panic>", "\tb.ne LBB0_1", "\tbrk #0x1", "LBB0_1:", "\tret"]
        n, br, ca, pa, bo = asm.stats("f", body)
        self.assertEqual((n, br, ca), (5, 2, 1))
        self.assertEqual(pa, 1)
        self.assertEqual(bo, 1)


if __name__ == "__main__":
    unittest.main()
