use crate::{
    analysis::{
        bounds::Bounds, conversion_type::ConversionType, function_parameters::CParameter,
        rust_type::RustType,
    },
    env::Env,
    traits::*,
};

pub trait ToParameter {
    fn to_parameter(&self, env: &Env, bounds: &Bounds) -> String;
}

impl ToParameter for CParameter {
    fn to_parameter(&self, env: &Env, bounds: &Bounds) -> String {
        if self.instance_parameter {
            format!("{}self", self.ref_mode.for_rust_type())
        } else {
            let type_str = match bounds.get_parameter_bound(&self.name) {
                Some(bound) => bound.full_type_parameter_reference(self.ref_mode, self.nullable),
                None => {
                    let type_name = RustType::builder(env, self.typ)
                        .direction(self.direction)
                        .nullable(self.nullable)
                        .ref_mode(self.ref_mode)
                        .scope(self.scope)
                        .try_from_glib(&self.try_from_glib)
                        .try_build_param()
                        .into_string();
                    match ConversionType::of(env, self.typ) {
                        ConversionType::Unknown => format!("/*Unknown conversion*/{}", type_name),
                        _ => type_name,
                    }
                }
            };
            format!("{}: {}", self.name, type_str)
        }
    }
}
