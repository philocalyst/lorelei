//! wgsl-rs module (src/shaders/fused.rs) -> WGSL -> naga (validated) -> per entry point:
//! * SPIR-V 1.3 for the Vulkan presenter (`OUT_DIR/<entry>.spv`): bindings as declared (set 0: region head, columns,
//!   bins, counts, surfaces, certificates, atlases; set 1: the slot);
//! * GLSL 4.30 core for the OpenGL presenter (`OUT_DIR/<entry>.comp`): the GL presenter keeps the whole region in one
//!   buffer, so the column readers (`c_*`, the only readers of the `C_*` bindings) are rewritten to read the region at
//!   the header's byte offsets before naga sees the module, and the column bindings disappear. GL binding points:
//!   storage buffers 0 (region), 1 (bins), 2 (counts), 3..6 (surfaces), 7 (certificates), 8 (glyph atlas), 9 (image
//!   atlas); uniform block 0 (the slot).
#[path = "src/shaders/fused.rs"]
#[allow(dead_code, unused)]
mod fused_src;

use std::collections::BTreeMap;

const ENTRIES: [&str; 3] = ["fs_copy", "fs_bin", "fs_tile"];

/// (column binding name, header word holding its byte offset in the region): `FHdr`'s `*_off` fields.
const COLUMNS: [(&str, u32); 11] = [
    ("C_INST", 4),
    ("C_SPACES", 5),
    ("C_CLIPS", 6),
    ("C_DIR_ID", 7),
    ("C_DIR_INK", 8),
    ("C_PAGES", 10),
    ("C_GX", 11),
    ("C_GENT", 12),
    ("C_RUNS", 13),
    ("C_TOKS", 14),
    ("C_XFORMS", 25),
];

fn validate(wgsl: &str, what: &str) -> (naga::Module, naga::valid::ModuleInfo) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|e| panic!("{what}: {}", e.emit_to_string(wgsl)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{what}: naga validation: {e:?}"));
    (module, info)
}

/// SPIR-V without `ArrayStride` on the array types of workgroup variables: naga decorates every array type, and
/// Vulkan 1.3's SPIR-V environment forbids an explicit layout in the Workgroup storage class (VUID-StandaloneSpirv-
/// None-10684; drivers ignore it, the validation layer reports it). Only types no other storage class uses lose it.
fn strip_workgroup_strides(words: Vec<u32>) -> Vec<u32> {
    const OP_TYPE_POINTER: u32 = 32;
    const OP_VARIABLE: u32 = 59;
    const OP_DECORATE: u32 = 71;
    const ARRAY_STRIDE: u32 = 6;
    const WORKGROUP: u32 = 4;
    // (opcode, words) of every instruction after the header
    let mut insts: Vec<(u32, &[u32])> = Vec::new();
    let mut i = 5;
    while i < words.len() {
        let n = ((words[i] >> 16) as usize).max(1);
        insts.push((words[i] & 0xffff, &words[i..i + n]));
        i += n;
    }
    // pointer type id -> (storage class, pointee)
    let mut ptrs = std::collections::HashMap::new();
    for &(op, w) in &insts {
        if op == OP_TYPE_POINTER {
            ptrs.insert(w[1], (w[2], w[3]));
        }
    }
    let (mut wg, mut other) = (std::collections::HashSet::new(), std::collections::HashSet::new());
    for &(op, w) in &insts {
        if op == OP_VARIABLE {
            if let Some(&(sc, ty)) = ptrs.get(&w[1]) {
                if sc == WORKGROUP { wg.insert(ty) } else { other.insert(ty) };
            }
        }
    }
    for (_, &(sc, ty)) in &ptrs {
        if sc != WORKGROUP {
            other.insert(ty);
        }
    }
    let mut out = words[..5].to_vec();
    for &(op, w) in &insts {
        if op == OP_DECORATE && w.len() == 4 && w[2] == ARRAY_STRIDE && wg.contains(&w[1]) && !other.contains(&w[1]) {
            continue;
        }
        out.extend_from_slice(w);
    }
    out
}

/// The subgroup-free form (GL drivers without `KHR_shader_subgroup`, llvmpipe among them): every invocation is its
/// own one-lane subgroup (lane 0, subgroup `li`, 256 subgroups), the ordered prefix sum becomes a workgroup-memory
/// scan (Hillis-Steele, 8 steps), and a subgroup maximum of one lane is the value itself.
fn wg_form(wgsl: &str) -> String {
    let mut s = wgsl.to_string();
    let start = s.find("fn prefix(").expect("prefix");
    let end = start + s[start..].find("\n}\n").expect("prefix end") + 3;
    s.replace_range(
        start..end,
        "fn prefix(v: u32, lane: u32, sg: u32, nsg: u32) -> vec2u {
    SCAN[sg] = v;
    workgroupBarrier();
    var d = 1u;
    while d < 256u {
        var t = SCAN[sg];
        if sg >= d {
            t += SCAN[sg - d];
        }
        workgroupBarrier();
        SCAN[sg] = t;
        workgroupBarrier();
        d = d * 2u;
    }
    let incl = SCAN[sg];
    let total = SCAN[255u];
    workgroupBarrier();
    return vec2u(incl - v, total);
}
",
    );
    for decl in ["@builtin(subgroup_invocation_id) lane: u32,", "@builtin(subgroup_id) sg: u32,", "@builtin(num_subgroups) nsg: u32,"] {
        s = s.lines().filter(|l| l.trim() != decl).map(|l| format!("{l}\n")).collect();
    }
    for entry in ["fn fs_bin(", "fn fs_tile("] {
        let at = s.find(entry).expect("entry point");
        let body = at + s[at..].find(") {\n").expect("entry body") + 4;
        s.insert_str(body, "    let lane = 0u;\n    let sg = li;\n    let nsg = 256u;\n");
    }
    s = s.replace("subgroupMax(", "(");
    s = s.replace("var<workgroup> SGC: array<u32, 64>;", "var<workgroup> SGC: array<u32, 256>;\nvar<workgroup> SCAN: array<u32, 256>;");
    s = s.replace("var<workgroup> FLO: array<u32, 64>;", "var<workgroup> FLO: array<u32, 256>;");
    assert!(!s.contains("subgroup"), "the subgroup-free form still uses subgroups");
    s
}

/// The GL form of the module: every `C_X[i]` read becomes `POOL[(POOL[k] >> 2u) + i]` (k: the header word of column
/// X's byte offset), and the column declarations go.
fn gl_form(wgsl: &str) -> String {
    let mut out = String::with_capacity(wgsl.len());
    for line in wgsl.lines() {
        let decl = line.contains("var<storage") && COLUMNS.iter().any(|(n, _)| line.contains(&format!(" {n}:")) || line.contains(&format!(">{n}:")) || line.contains(&format!("> {n}:")));
        if decl {
            continue;
        }
        let mut l = line.to_string();
        for (n, k) in COLUMNS {
            l = l.replace(&format!("{n}["), &format!("POOL[(POOL[{k}u] >> 2u) + "));
        }
        out.push_str(&l);
        out.push('\n');
    }
    for (n, _) in COLUMNS {
        assert!(!out.contains(n), "the GL form still names column {n}: {out}");
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/shaders/fused.rs");
    let out = std::env::var("OUT_DIR").unwrap();
    let wgsl = fused_src::fused::WGSL_SOURCE.wgsl_source().unwrap_or_else(|e| panic!("wgsl-rs: {e}"));
    std::fs::write(format!("{out}/fused.wgsl"), &wgsl).unwrap();
    let (module, info) = validate(&wgsl, "fused.wgsl");

    // SPIR-V 1.3 (subgroup arithmetic); runtime array lengths are SPIR-V's own; every index the kernels compute is
    // masked or clamped by construction, so no bounds checks are added
    for e in ENTRIES {
        let options = naga::back::spv::Options {
            lang_version: (1, 3),
            flags: naga::back::spv::WriterFlags::empty(),
            bounds_check_policies: naga::proc::BoundsCheckPolicies::default(),
            zero_initialize_workgroup_memory: naga::back::spv::ZeroInitializeWorkgroupMemoryMode::None,
            ..Default::default()
        };
        let pipeline = naga::back::spv::PipelineOptions { shader_stage: naga::ShaderStage::Compute, entry_point: e.to_string() };
        let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline)).unwrap_or_else(|err| panic!("naga spv {e}: {err:?}"));
        let words = strip_workgroup_strides(words);
        std::fs::write(format!("{out}/{e}.spv"), words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
    }

    let gl = gl_form(&wgsl);
    std::fs::write(format!("{out}/fused_gl.wgsl"), &gl).unwrap();
    let glwg = wg_form(&gl);
    std::fs::write(format!("{out}/fused_gl_wg.wgsl"), &glwg).unwrap();
    let mut map = BTreeMap::new();
    let mut put = |binding: u32, slot: u8| {
        map.insert(naga::ResourceBinding { group: 0, binding }, slot);
    };
    put(0, 0);
    put(12, 1);
    put(13, 2);
    for s in 0..4 {
        put(14 + s, 3 + s as u8);
    }
    put(18, 7);
    put(19, 8);
    put(20, 9);
    map.insert(naga::ResourceBinding { group: 1, binding: 0 }, 0);
    let options = naga::back::glsl::Options {
        version: naga::back::glsl::Version::Desktop(430),
        writer_flags: naga::back::glsl::WriterFlags::empty(),
        binding_map: map,
        zero_initialize_workgroup_memory: false,
    };
    for (form, src, suffix) in [("fused_gl.wgsl", &gl, ""), ("fused_gl_wg.wgsl", &glwg, "_wg")] {
    let (gmod, ginfo) = validate(src, form);
    for e in ENTRIES {
        let pipeline = naga::back::glsl::PipelineOptions { shader_stage: naga::ShaderStage::Compute, entry_point: e.to_string(), multiview: None };
        let mut text = String::new();
        let w = naga::back::glsl::Writer::new(&mut text, &gmod, &ginfo, &options, &pipeline, naga::proc::BoundsCheckPolicies::default());
        w.unwrap_or_else(|err| panic!("naga glsl {e}: {err:?}")).write().unwrap_or_else(|err| panic!("naga glsl {e}: {err:?}"));
        // naga asks for every subgroup extension once any subgroup operation is used; the kernels use the basic
        // builtins and arithmetic only (a driver without shuffles or quads still runs them)
        let unused = ["vote", "ballot", "shuffle", "shuffle_relative", "quad"].map(|x| format!("#extension GL_KHR_shader_subgroup_{x} : require"));
        let text: String = text.lines().filter(|l| !unused.iter().any(|u| l.trim() == u)).map(|l| format!("{l}\n")).collect();
        std::fs::write(format!("{out}/{e}{suffix}.comp"), text).unwrap();
    }
    }
}
