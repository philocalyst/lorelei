//! Integer colour functions with the resource reads declared by their paint operands.
use crate::paint::PCode;
use wgsl_rs::std::Vec4u;

pub type ColourFn = fn(i32, i32, u32, Vec4u, Vec4u) -> Vec4u;

pub struct Module {
    pub entry: &'static str,
    pub source: &'static wgsl_rs::Source,
    pub cpu: ColourFn,
}
impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module")
            .field("entry", &self.entry)
            .finish()
    }
}

/// Bind a typed colour function and the WGSL source that defines it. The CPU function takes the same
/// integer arguments and returns the same words as the named WGSL function.
pub trait Program: 'static {
    const MODULE: &'static Module;
}

fn collect(code: &'static PCode, modules: &mut Vec<&'static Module>) {
    use crate::paint::Glue;
    match code {
        PCode::Prim {
            glue: Glue::Author(module),
            ..
        } => {
            if let Some(previous) = modules.iter().find(|p| p.entry == module.entry) {
                assert!(
                    std::ptr::eq(*previous, *module),
                    "duplicate author entry {}",
                    module.entry
                );
            } else {
                modules.push(module);
            }
        }
        PCode::Over { a, b, .. } => {
            collect(a, modules);
            collect(b, modules);
        }
        PCode::Clip { p, .. } | PCode::Hit { p, .. } => collect(p, modules),
        PCode::Prim { .. } => {}
    }
}

fn bounded(body: &naga::Block) -> bool {
    use naga::Statement;
    body.iter().all(|s| match s {
        Statement::Loop { .. } | Statement::ControlBarrier(_) | Statement::MemoryBarrier(_) | Statement::Kill => false,
        Statement::Block(block) => bounded(block),
        Statement::If { accept, reject, .. } => bounded(accept) && bounded(reject),
        Statement::Switch { cases, .. } => cases.iter().all(|c| bounded(&c.body)),
        _ => true,
    })
}

impl Module {
    /// Validate the independently compiled module before it is joined to the generated renderer.
    pub fn validate(&self) -> Result<(), String> {
        self.parse().map(|_| ())
    }

    fn parse(&self) -> Result<(String, naga::Module, naga::valid::ModuleInfo), String> {
        use naga::{ScalarKind, TypeInner, VectorSize};
        if !self.entry.starts_with("fx_")
            || self.entry.ends_with(|c: char| c.is_ascii_digit())
            || !self
                .entry
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("an author entry must start with fx_ and end in a non-digit".into());
        }
        let source = self.source.wgsl_source().map_err(|e| e.to_string())?;
        if source.len() > 16 * 1024 {
            return Err("author module exceeds 16 KiB".into());
        }
        let module =
            naga::front::wgsl::parse_str(&source).map_err(|e| e.emit_to_string(&source))?;
        if !module.entry_points.is_empty()
            || !module.global_variables.is_empty()
            || !module.constants.is_empty()
            || !module.overrides.is_empty()
        {
            return Err(
                "author modules must contain only integer colour functions and their local state"
                    .into(),
            );
        }
        let integer = |k| matches!(k, ScalarKind::Sint | ScalarKind::Uint | ScalarKind::Bool);
        if module.types.iter().any(|(_, ty)| match ty.inner {
            TypeInner::Scalar(scalar) | TypeInner::Vector { scalar, .. } => !integer(scalar.kind),
            _ => true,
        }) || module.functions.iter().any(|(_, f)| {
            !bounded(&f.body)
                || !f
                    .name
                    .as_deref()
                    .is_some_and(|name| name.starts_with(self.entry))
        }) {
            return Err(
                "author modules require bounded integer functions named within the entry namespace"
                    .into(),
            );
        }
        let (_, entry) = module
            .functions
            .iter()
            .find(|(_, f)| f.name.as_deref() == Some(self.entry))
            .ok_or_else(|| "author entry missing".to_string())?;
        let actual: Vec<_> = entry
            .arguments
            .iter()
            .map(|arg| &module.types[arg.ty].inner)
            .collect();
        let scalar = |ty: &TypeInner, kind| matches!(ty, TypeInner::Scalar(s) if s.kind == kind && s.width == 4);
        let colour = |ty: &TypeInner| matches!(ty, TypeInner::Vector { size: VectorSize::Quad, scalar } if scalar.kind == ScalarKind::Uint && scalar.width == 4);
        if !matches!(actual.as_slice(), [x, y, t, a, b] if scalar(x, ScalarKind::Sint) && scalar(y, ScalarKind::Sint) && scalar(t, ScalarKind::Uint) && colour(a) && colour(b))
            || !entry
                .result
                .as_ref()
                .is_some_and(|r| colour(&module.types[r.ty].inner))
        {
            return Err("author entry has the wrong integer colour signature".into());
        }
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|e| e.to_string())?;
        Ok((source, module, info))
    }
}

pub(crate) fn append(descs: &[&crate::scene::Desc], out: &mut String, metal: bool) {
    let mut programs = Vec::new();
    for d in descs {
        collect(d.code, &mut programs);
    }
    for program in programs {
        let (wgsl, module, info) = program
            .parse()
            .unwrap_or_else(|e| panic!("{}: {e}", program.entry));
        out.push('\n');
        if metal {
            let options = naga::back::msl::Options {
                lang_version: (3, 0),
                ..Default::default()
            };
            let (msl, _) =
                naga::back::msl::write_string(&module, &info, &options, &Default::default())
                    .expect("author MSL translation");
            out.push_str(&msl);
        } else {
            out.push_str(&wgsl);
        }
    }
}
