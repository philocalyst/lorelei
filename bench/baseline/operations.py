"""Read each fixture's operation counters by column or field name."""

import re


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


def guibench(output, op):
    tag = "mount" if op == "mount" else "result"
    lines = [line for line in output.splitlines() if line.startswith(tag + " ")]
    if len(lines) != 1:
        raise ValueError(f"expected one {tag!r} counter line")
    fields = dict(re.findall(r"\b(\w+)=([^\s]+)", lines[0]))
    if tag == "result" and fields.get("op") != op:
        raise ValueError("unexpected façade operation")
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


def parse(kind, stdout, stderr, args, env):
    if kind in TABLES:
        return table(kind, stdout, env)
    if kind == "guibench":
        return guibench(stdout + "\n" + stderr, args[args.index("--op") + 1])
    if kind == "effects":
        return effects(stdout, args)
    if kind == "pbench":
        if not re.search(r"^# cross-check failures: 0;", stdout, re.M):
            raise ValueError("pbench cross-check did not pass")
        return {}
    raise ValueError(f"unknown fixture parser {kind!r}")
