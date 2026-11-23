"""Read each fixture's operation counters by column or field name."""

from decimal import Decimal
import re
import statistics


TABLES = {
    "sheet": ("scene", ("phase",), {"instr_p50", "instr_p95", "cycles_p50", "tiles_p50"}),
    "text": ("scene", ("scene", "phase"), {"instr_p50", "instr_p95", "cycles_p50", "tiles_p50"}),
    "anim": ("scene", ("phase",), {"instr_p50", "cycles_p50", "tiles_p50"}),
    "ops": ("scene", ("part", "phase"), {"instr_p50", "instr_p95", "cycles_p50"}),
    "damage": ("scene", ("phase",), {"instr_p50", "instr_p95", "cycles_p50", "tiles_p50"}),
    "events": ("rows", ("step",), {
        "dispatch_instr", "step_instr", "dispatch_allocs", "step_cycles_warm", "step_cycles_cold",
        "cold_minus_warm",
    }),
}

REQUIRED = {
    "sheet": {"floor", "idle", "hover", "select", "edit_exposed", "theme"},
    "text": {"rows/mount", "rows/idle", "rows/hover"},
    "anim": {"frame_endless", "fade_start_turn", "pointer_words", "scroll_3_rows"},
    "damage": {"floor", "idle", "hover", "under_glass", "under_glass_gated", "move", "scroll", "theme", "recommit",
               "report"},
    "events": {"pointer_onto_row", "pointer_same_row", "click_row", "key_unfocused", "text_focused_field"},
}


def unsigned(value):
    if not re.fullmatch(r"[0-9]+", value):
        raise ValueError(f"invalid counter: {value!r}")
    return int(value)


def table(kind, stdout, env):
    first, key_columns, required_columns = TABLES[kind]
    header = None
    operations = {}
    for line in stdout.splitlines():
        values = line.split("\t")
        if len(values) == 1 or line.startswith("#"):
            continue
        if values[0] == first:
            if header is not None or len(set(values)) != len(values):
                raise ValueError("repeated or ambiguous operation header")
            header = values
            if not required_columns.union(key_columns) <= set(header):
                raise ValueError("missing operation column")
            continue
        if header is None or len(values) != len(header):
            raise ValueError("row does not match operation header")
        row = dict(zip(header, values))
        key = "/".join(row.pop(name) for name in key_columns)
        if kind == "events":
            if row.pop("rows") != env["ROWS"]:
                raise ValueError("unexpected logical row count")
        else:
            scene = row.pop("scene", None)
            # (`{kind}_x`: the fixture's name before the rename, in output from an older checkout)
            if scene is not None and kind != "text" and scene not in (kind, f"{kind}_x"):
                raise ValueError("unexpected fixture name")
            if unsigned(row["steps"]) == 0:
                raise ValueError("zero operation samples")
        if key in operations and kind == "ops" and env["PART"] == "search":
            # The probe clears both query strings back to the same empty needle.
            key += " (second query)"
        if not key or key in operations:
            raise ValueError(f"empty or repeated operation name: {key!r}")
        operations[key] = {name: unsigned(value) for name, value in row.items()}
    missing = REQUIRED.get(kind, set()) - operations.keys()
    if missing:
        raise ValueError(f"missing operations: {', '.join(sorted(missing))}")
    if kind == "ops" and any(not key.startswith(env["PART"] + "/") for key in operations):
        raise ValueError("unexpected incremental-operators part")
    primary = "dispatch_instr" if kind == "events" else "instr_p50"
    if not operations or not any(row[primary] for row in operations.values()):
        raise ValueError("empty operation counters or PMU unavailable")
    return operations


def guibench(output, args):
    op = args[args.index("--op") + 1]
    blocked = "--blocks" in args and unsigned(args[args.index("--blocks") + 1]) != 0
    tag = "mount" if op == "mount" else "perop" if blocked else "result"
    lines = [line for line in output.splitlines() if line.startswith(tag + " ")]
    if len(lines) != 1:
        raise ValueError(f"expected one {tag!r} counter line")
    fields = dict(re.findall(r"\b(\w+)=([^\s]+)", lines[0]))
    if tag != "mount" and (fields.get("op") != op or fields.get("render") != "false"):
        raise ValueError("unexpected façade operation or renderer")
    if tag == "perop":
        median, lo, hi = (unsigned(fields[name]) for name in ("med", "lo", "hi"))
        if not 0 < lo <= median <= hi:
            raise ValueError("invalid façade operation bounds")
        return {op: {"instructions": median, "instructions_lo": lo, "instructions_hi": hi,
                     "records": unsigned(fields["records"]), "frames": unsigned(fields["frames"])}}
    counters = {name: unsigned(fields[name]) for name in ("instructions", "rss", "footprint")}
    if counters["instructions"] == 0:
        raise ValueError("façade instruction counter unavailable")
    return {op: counters}


def effects(stdout, args):
    mode, cards, cell, samples = args
    if mode == "cold":
        prefix = "spawn_to_first"
        labels = ("wall_ns", "instructions", "cpu_ns", "rss", "footprint")
        skip = 2
        identity = [cards]
    else:
        prefix = "summary"
        labels = ("draws", "wall_p50_ns", "wall_p95_ns", "wall_p99_ns",
                  "instructions", "cpu_ns", "rss", "footprint")
        skip = 4
        identity = [cards, cell, samples]
    lines = [line.split(",") for line in stdout.splitlines() if line.startswith(prefix + ",")]
    if len(lines) != 1 or lines[0][1:skip] != identity:
        raise ValueError("missing or mismatched effects summary")
    fields = lines[0][skip:]
    if len(fields) != len(labels) + (mode != "cold"):
        raise ValueError("malformed effects summary")
    counters = {name: unsigned(value) for name, value in zip(labels, fields)}
    if counters["instructions"] == 0 and cell != "idle":
        raise ValueError("effects instruction counter unavailable")
    return {cell: counters}


def effects_core(stdout, args):
    _, cards, cell = args
    startup = []
    steps = []
    idle = []
    for line in stdout.splitlines():
        fields = line.split(",")
        if fields[0] == "core_startup":
            if fields[1:4] != [cards, cell, "4"] or len(fields) != 8:
                raise ValueError("mismatched typed-core effect startup")
            startup.append(dict(zip(("wall_ns", "instructions", "rss", "footprint"), map(unsigned, fields[4:]))))
        elif fields[0] == "core_step":
            if fields[1:5] != [cards, cell, "4", str(len(steps))] or len(fields) != 11:
                raise ValueError("mismatched typed-core effect step")
            labels = ("wall_ns", "instructions", "cpu_ns", "rss", "footprint", "tiles")
            steps.append(dict(zip(labels, map(unsigned, fields[5:]))))
        elif fields[0] == "core_idle":
            if fields[1] != cards or len(fields) != 5:
                raise ValueError("mismatched typed-core effect idle")
            idle.append(dict(zip(("instructions", "rss", "footprint"), map(unsigned, fields[2:]))))
    if len(startup) != 1 or startup[0]["instructions"] == 0:
        raise ValueError("missing typed-core startup counters")
    if cell == "startup" and not idle and not steps:
        return {cell: startup[0]}
    if cell == "idle" and len(idle) == 1 and not steps:
        return {cell: idle[0]}
    if cell not in ("startup", "idle") and len(steps) == 20 and not idle:
        return {cell: {key: statistics.median(row[key] for row in steps) for key in steps[0]}}
    raise ValueError("incomplete typed-core effect trace")


# The phases `crates/bough-paint/examples/fxbench.rs` runs per scene (its `cells` table, without PALETTE=1).
FX_COMMON = ("idle", "one spring", "all springs", "hover above glass", "hover beneath glass", "glass moves 1 px")
FX_PHASES = {
    "effects": (*FX_COMMON, "scroll 11 px", "theme"),
    "pane": FX_COMMON,
    "media": ("idle", "one video frame", "video under glass", "all videos (every 4th)",
              "a/v playback (audio clocks)", "scroll 11 px", "hover above glass", "theme", "all springs",
              "glass over playing video"),
}
# Every executor is gated on its frame-thread instructions. `metal` and `mixed` (the per-change chooser) count
# the encoding thread's share of a GPU frame. In three replays of one binary both spread up to 5% on
# rows near 100k instructions, against 0.1% for `cpu1-nocache`. `cpu1` spreads up to 8% only on rows under
# 10k instructions. Unlike pbench's pooled executors, no worker thread takes a schedule-dependent share of the
# frame, so these rows stay primary. A `mixed` row also records how many measured frames the chooser ran idle,
# on the CPU and on the GPU: a decision that flips under load shows there and in the instruction count.
FX_EXECUTORS = ("cpu1", "cpu1-nocache", "metal", "mixed")
FX_HEADER = ("scene", "n", "cell", "executor", "tiles/frame", "recs/frame", "prefix/frame", "blocks/frame",
             "layer_px/frame", "layers_px", "active", "instr_med_K", "instr_min_K", "instr_mean_K",
             "wall_med_us", "gpu_med_us", "mismatch", "turn_med", "ran_idle/cpu/gpu", "prefix_same/prefix",
             "blocks_same/blocks by level 1..5", "ranks/slots")
# The plain numeric columns, recorded per operation.
FX_METRICS = FX_HEADER[4:16]


def fraction(value, parts, ordered=True):
    """`a/b[/c]` diagnostics: unsigned parts, each `same` no larger than its total when `ordered`."""
    fields = value.split("/")
    if len(fields) != parts:
        raise ValueError(f"invalid staged-paint diagnostic: {value!r}")
    numbers = [unsigned(field) for field in fields]
    if ordered and numbers[0] > numbers[1]:
        raise ValueError(f"staged-paint diagnostic out of order: {value!r}")
    return numbers


def staged_paint(stdout, args, env):
    scene, size, *mode = args
    dense = mode == ["dense"]
    expected_size = size + ("d" if dense else "")
    reps = unsigned(env["REPS"])
    phases = {f"{executor}/{phase}" for phase in FX_PHASES[scene] for executor in FX_EXECUTORS}
    identity = f"# fxbench {scene} {size}{' dense' if dense else ''}; 1280x800; reps {reps};"
    if not stdout.startswith(identity):
        raise ValueError("unexpected staged-paint scene, size or repetitions")
    header = None
    result = {}
    for line in stdout.splitlines():
        if line.startswith("#") or "\t" not in line:
            continue
        fields = line.split("\t")
        if fields[0] == "scene":
            if header is not None or tuple(fields) != FX_HEADER:
                raise ValueError("repeated or unexpected staged-paint header")
            header = fields
            continue
        if header is None or len(fields) != len(header):
            raise ValueError("malformed staged-paint row")
        row = dict(zip(header, fields))
        if row["scene"] != scene or row["n"] != expected_size or row["mismatch"] != "0":
            raise ValueError("unexpected staged-paint scene or pixel mismatch")
        executor = row["executor"]
        key = f"{executor}/{row['cell']}"
        if key not in phases or key in result:
            raise ValueError(f"unexpected or duplicate staged-paint operation: {key}")
        if any(not re.fullmatch(r"[0-9]+(?:\.[0-9]+)?", row[name]) for name in FX_METRICS):
            raise ValueError(f"invalid staged-paint metric in {key}")
        # a count of records, not a per-frame mean
        unsigned(row["active"])
        # The composite diagnostics are checked and left in the raw log, except the chooser's split.
        if (row["turn_med"] == "-") != (executor == "mixed"):
            raise ValueError(f"staged-paint turn counter present or absent unexpectedly in {key}")
        if executor != "mixed":
            unsigned(row["turn_med"])
        ran = fraction(row["ran_idle/cpu/gpu"], 3, ordered=False)
        if (sum(ran) != reps) if executor == "mixed" else any(ran):
            raise ValueError(f"staged-paint chooser split does not account for the frames in {key}")
        fraction(row["prefix_same/prefix"], 2)
        levels = row["blocks_same/blocks by level 1..5"].split(" ")
        if len(levels) != 5:
            raise ValueError(f"staged-paint block levels malformed in {key}")
        for level in levels:
            fraction(level, 2)
        # the resident plan's rank levels and slots: every level holds a slot
        fraction(row["ranks/slots"], 2)
        instructions = int(1000 * Decimal(row["instr_med_K"]))
        if instructions == 0:
            raise ValueError("staged-paint instruction counter unavailable")
        counters = {"instructions": instructions,
                    **{name: float(row[name]) for name in FX_METRICS if name != "instr_med_K"}}
        if executor == "mixed":
            counters.update(zip(("ran_idle", "ran_cpu", "ran_gpu"), ran))
        result[key] = counters
    if result.keys() != phases or not re.search(r"^# mismatching words in total: 0;", stdout, re.M):
        raise ValueError("missing staged-paint operations or retained-pixel verdict")
    return result


FLAT_CELLS = {
    "cards": ("idle", "restyle one card", "restyle a card beneath the pane", "one card on a spring", "scroll 11 px",
              "pane moves 1 px"),
    "pages": ("pages idle", "page cross-fade", "page slide", "page blur-reveal", "one card of a page"),
}
FLAT_HEADER = ("scene", "cell", "exec", "ms/frame (median)", "ms (min)", "k instr/frame", "tiles/frame",
               "records/frame", "group tiles/frame", "group blocks/frame", "mismatches")
FLAT_EXECUTORS = ("Cpu1", "Cpu8", "Metal")
# `Cpu8` runs the tile kernel with seven extra workers: the frame thread's share of the frame depends on
# scheduling (three replays of one binary spread 1.7% on the page slide), as with pbench's pooled executors.
# Its rows must be present and pixel-exact. Their counters stay in the raw log. `Metal` counts the encoding
# thread. Its replays spread under 1% apart from the 55k-instruction idle frames (7%), so it is gated as
# fxbench's Metal rows are.
FLAT_PRIMARY = ("Cpu1", "Metal")


def flatbench(stdout, args, env):
    (cards,) = args
    reps = unsigned(env["REPS"])
    lines = stdout.splitlines()
    if not lines or not re.match(rf"# flatbench: {cards} cards \([0-9]+ records\) .*; {reps} frames per cell$", lines[0]):
        raise ValueError("unexpected flatbench scene size or repetitions")
    expected = {f"{executor}/{cell}" for cells in FLAT_CELLS.values() for cell in cells for executor in FLAT_EXECUTORS}
    header = None
    seen = set()
    operations = {}
    for line in lines:
        if line.startswith("#") or "\t" not in line:
            continue
        fields = line.split("\t")
        if fields[0] == "scene":
            if header is not None or tuple(fields) != FLAT_HEADER:
                raise ValueError("repeated or unexpected flatbench header")
            header = fields
            continue
        if header is None or len(fields) != len(header):
            raise ValueError("malformed flatbench row")
        row = dict(zip(header, fields))
        key = f"{row['exec']}/{row['cell']}"
        if row["cell"] not in FLAT_CELLS.get(row["scene"], ()) or key not in expected or key in seen:
            raise ValueError(f"unexpected or repeated flatbench row: {key}")
        if row["mismatches"] != "0":
            raise ValueError(f"flatbench pixel mismatch in {key}")
        seen.add(key)
        if any(not re.fullmatch(r"[0-9]+\.[0-9]+", row[name]) for name in FLAT_HEADER[3:10]):
            raise ValueError(f"invalid flatbench metric in {key}")
        if row["exec"] in FLAT_PRIMARY:
            instructions = int(Decimal(row["k instr/frame"]) * 1000)
            if instructions == 0:
                raise ValueError(f"flatbench PMU unavailable for {key}")
            operations[key] = {"instructions": instructions, **{name: float(row[name]) for name in FLAT_HEADER[6:10]},
                               "wall_med_ms": float(row["ms/frame (median)"]), "wall_min_ms": float(row["ms (min)"])}
    if seen != expected or lines[-1] != "# mismatching words in total: 0":
        raise ValueError("missing flatbench rows or retained-pixel verdict")
    return operations


PBENCH_EXECUTORS = {"A1", "A", "AP", "AG", "AD", "K", "K1", "KP", "KG"}
# A frame-thread counter includes a variable share of the work for pooled and Metal paths.
PBENCH_PRIMARY = {"A1", "A", "K", "K1"}


def pbench(stdout, args, env):
    scene, size = args
    phases = {"full", "theme", "time"}
    if scene == "canvas":
        phases.update(("hover", "hover1", "pan", "drag"))
    phases = {f"{scene}.{phase}" for phase in phases}
    identity = f"# pbench {scene} Some({size}); reps {env['REPS']}; workers {env['WORKERS']};"
    lines = stdout.splitlines()
    if not lines or not lines[0].startswith(identity):
        raise ValueError("unexpected pbench scene, repetitions or worker count")
    if not lines[-1].startswith("# cross-check failures: 0;"):
        raise ValueError("pbench cross-check did not pass")
    header = None
    seen = set()
    operations = {}
    for line in lines:
        if line.startswith("#") or "\t" not in line:
            continue
        fields = line.split("\t")
        if fields[0] == "phase":
            if (header is not None or len(fields) != len(set(fields)) or
                    not {"executor", "tiles", "thread_Kinstr"} <= set(fields)):
                raise ValueError("missing or ambiguous pbench columns")
            header = fields
            continue
        if header is None or len(fields) != len(header):
            raise ValueError("malformed pbench row")
        row = dict(zip(header, fields))
        phase, executor = row["phase"], row["executor"].split(maxsplit=1)[0]
        key = f"{phase}/{executor}"
        if (phase not in phases or
                executor not in PBENCH_EXECUTORS or key in seen):
            raise ValueError(f"unexpected or repeated pbench row: {key}")
        seen.add(key)
        value = row["thread_Kinstr"]
        if not re.fullmatch(r"[0-9]+\.[0-9]", value) or not Decimal(value):
            raise ValueError(f"pbench PMU unavailable for {key}")
        if executor in PBENCH_PRIMARY:
            operations[key] = {"instructions": int(Decimal(value) * 1000),
                               "tiles": unsigned(row["tiles"])}
    if len(seen) != len(phases) * len(PBENCH_EXECUTORS):
        raise ValueError("missing pbench operation rows")
    return operations


def parse(kind, stdout, stderr, args, env):
    if kind in TABLES:
        return table(kind, stdout, env)
    if kind == "guibench":
        return guibench(stdout + "\n" + stderr, args)
    if kind == "effects":
        return effects(stdout, args)
    if kind == "effects-core":
        return effects_core(stdout, args)
    if kind == "fxbench":
        return staged_paint(stdout, args, env)
    if kind == "pbench":
        return pbench(stdout, args, env)
    if kind == "flatbench":
        return flatbench(stdout, args, env)
    raise ValueError(f"unknown fixture parser {kind!r}")
