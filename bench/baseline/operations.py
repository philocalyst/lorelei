"""Read each fixture's operation counters by column or field name."""

from decimal import Decimal
import re
import statistics


TABLES = {
    "sheet": ("scene", ("phase",), {"instr_p50", "instr_p95", "cycles_p50", "tiles_p50"}),
    "text": ("scene", ("scene", "phase"), {"instr_p50", "instr_p95", "cycles_p50", "tiles_p50"}),
    "anim": ("scene", ("phase",), {"instr_p50", "cycles_p50", "tiles_p50"}),
    "ops": ("scene", ("part", "phase"), {"instr_p50", "instr_p95", "cycles_p50"}),
    "events": ("rows", ("step",), {
        "dispatch_instr", "step_instr", "dispatch_allocs", "step_cycles_warm", "step_cycles_cold",
        "cold_minus_warm",
    }),
}

REQUIRED = {
    "sheet": {"floor", "idle", "hover", "select", "edit_exposed", "theme"},
    "text": {"rows/mount", "rows/idle", "rows/hover"},
    "anim": {"frame_endless", "fade_start_turn", "pointer_words", "scroll_3_rows"},
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
            if scene is not None and kind != "text" and scene != f"{kind}_x":
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


FX_COMMON = ("idle", "one spring", "all springs", "hover above glass", "hover beneath glass", "glass moves 1 px")
FX_PHASES = {
    "effects": (*FX_COMMON, "scroll 11 px", "theme"),
    "pane": FX_COMMON,
    "media": ("idle", "one video frame", "video under glass", "all videos (every 4th)",
              "scroll 11 px", "hover above glass", "theme", "all springs", "glass over playing video"),
}


def staged_paint(stdout, args):
    scene, size, *mode = args
    expected_size = size + ("d" if mode == ["dense"] else "")
    phases = {f"{executor}/{phase}" for phase in FX_PHASES[scene]
              for executor in ("cpu1", "cpu1-nocache", "metal")}
    header = None
    result = {}
    for line in stdout.splitlines():
        if line.startswith("#") or "\t" not in line:
            continue
        fields = line.split("\t")
        if fields[0] == "scene":
            if header is not None or len(fields) != len(set(fields)):
                raise ValueError("ambiguous staged-paint header")
            header = fields
            continue
        if header is None or len(fields) != len(header):
            raise ValueError("malformed staged-paint row")
        row = dict(zip(header, fields))
        if row.pop("scene") != scene or row.pop("n") != expected_size or row.pop("mismatch") != "0":
            raise ValueError("unexpected staged-paint scene or pixel mismatch")
        key = f"{row.pop('executor')}/{row.pop('cell')}"
        if key not in phases or key in result:
            raise ValueError(f"unexpected or duplicate staged-paint operation: {key}")
        if any(not re.fullmatch(r"[0-9]+(?:\.[0-9]+)?", v) for v in row.values()):
            raise ValueError("invalid staged-paint metric")
        instructions = int(1000 * Decimal(row.pop("instr_med_K")))
        if instructions == 0:
            raise ValueError("staged-paint instruction counter unavailable")
        result[key] = {"instructions": instructions, **{k: float(v) for k, v in row.items()}}
    if result.keys() != phases or not re.search(r"^# mismatching words in total: 0;", stdout, re.M):
        raise ValueError("missing staged-paint operations or retained-pixel verdict")
    return result


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
        return staged_paint(stdout, args)
    if kind == "pbench":
        if not re.search(r"^# cross-check failures: 0;", stdout, re.M):
            raise ValueError("pbench cross-check did not pass")
        return {}
    raise ValueError(f"unknown fixture parser {kind!r}")
