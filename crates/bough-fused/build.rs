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
        std::fs::write(format!("{out}/{e}.spv"), words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
    }

    let gl = gl_form(&wgsl);
    std::fs::write(format!("{out}/fused_gl.wgsl"), &gl).unwrap();
    let (gmod, ginfo) = validate(&gl, "fused_gl.wgsl");
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
    for e in ENTRIES {
        let pipeline = naga::back::glsl::PipelineOptions { shader_stage: naga::ShaderStage::Compute, entry_point: e.to_string(), multiview: None };
        let mut text = String::new();
        let w = naga::back::glsl::Writer::new(&mut text, &gmod, &ginfo, &options, &pipeline, naga::proc::BoundsCheckPolicies::default());
        w.unwrap_or_else(|err| panic!("naga glsl {e}: {err:?}")).write().unwrap_or_else(|err| panic!("naga glsl {e}: {err:?}"));
        // naga asks for every subgroup extension once any subgroup operation is used; the kernels use the basic
        // builtins and arithmetic only (a driver without shuffles or quads still runs them)
        let unused = ["vote", "ballot", "shuffle", "shuffle_relative", "quad"].map(|x| format!("#extension GL_KHR_shader_subgroup_{x} : require"));
        let text: String = text.lines().filter(|l| !unused.iter().any(|u| l.trim() == u)).map(|l| format!("{l}\n")).collect();
        std::fs::write(format!("{out}/{e}.comp"), text).unwrap();
    }
}
