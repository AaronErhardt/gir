use super::{
    conversion_type::ConversionType, out_parameters::can_as_return,
    override_string_type::override_string_type_parameter, ref_mode::RefMode, rust_type::RustType,
    try_from_glib::TryFromGlib,
};
use crate::{
    analysis::{self, bounds::Bounds},
    config::{self, parameter_matchable::ParameterMatchable},
    env::Env,
    library::{self, Nullable, ParameterScope, TypeId},
    nameutil,
    traits::IntoString,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Parameter {
    pub lib_par: library::Parameter,
    pub try_from_glib: TryFromGlib,
}

impl Parameter {
    pub fn from_parameter(
        env: &Env,
        lib_par: &library::Parameter,
        configured_parameters: &[&config::functions::Parameter],
    ) -> Self {
        Parameter {
            lib_par: lib_par.clone(),
            try_from_glib: TryFromGlib::from_parameter(env, lib_par.typ, configured_parameters),
        }
    }

    pub fn from_return_value(
        env: &Env,
        lib_par: &library::Parameter,
        configured_functions: &[&config::functions::Function],
    ) -> Self {
        Parameter {
            lib_par: lib_par.clone(),
            try_from_glib: TryFromGlib::from_return_value(env, lib_par.typ, configured_functions),
        }
    }
}

//TODO: remove unused fields
#[derive(Clone, Debug)]
pub struct RustParameter {
    pub ind_c: usize, //index in `Vec<CParameter>`
    pub name: String,
    pub typ: TypeId,
    pub allow_none: bool,
}

#[derive(Clone, Debug)]
pub struct CParameter {
    pub name: String,
    pub typ: TypeId,
    pub c_type: String,
    pub instance_parameter: bool,
    pub direction: library::ParameterDirection,
    pub nullable: library::Nullable,
    pub transfer: library::Transfer,
    pub caller_allocates: bool,
    pub is_error: bool,
    pub scope: ParameterScope,
    /// Index of the user data parameter associated with the callback.
    pub user_data_index: Option<usize>,
    /// Index of the destroy notification parameter associated with the callback.
    pub destroy_index: Option<usize>,

    //analysis fields
    pub ref_mode: RefMode,
    pub try_from_glib: TryFromGlib,
}

#[derive(Clone, Debug)]
pub enum TransformationType {
    ToGlibDirect {
        name: String,
    },
    ToGlibScalar {
        name: String,
        nullable: library::Nullable,
        needs_into: bool,
    },
    ToGlibPointer {
        name: String,
        instance_parameter: bool,
        transfer: library::Transfer,
        ref_mode: RefMode,
        //filled by functions
        to_glib_extra: String,
        explicit_target_type: String,
        pointer_cast: String,
        in_trait: bool,
        nullable: bool,
    },
    ToGlibBorrow,
    ToGlibUnknown {
        name: String,
    },
    Length {
        array_name: String,
        array_length_name: String,
        array_length_type: String,
    },
    IntoRaw(String),
    ToSome(String),
}

impl TransformationType {
    pub fn is_to_glib(&self) -> bool {
        use self::TransformationType::*;
        matches!(
            *self,
            ToGlibDirect { .. }
                | ToGlibScalar { .. }
                | ToGlibPointer { .. }
                | ToGlibBorrow
                | ToGlibUnknown { .. }
                | ToSome(_)
                | IntoRaw(_)
        )
    }

    pub fn set_to_glib_extra(&mut self, to_glib_extra_: &str) {
        if let TransformationType::ToGlibPointer { to_glib_extra, .. } = self {
            *to_glib_extra = to_glib_extra_.to_owned();
        }
    }
}

#[derive(Clone, Debug)]
pub struct Transformation {
    pub ind_c: usize,            //index in `Vec<CParameter>`
    pub ind_rust: Option<usize>, //index in `Vec<RustParameter>`
    pub transformation_type: TransformationType,
}

#[derive(Clone, Default, Debug)]
pub struct Parameters {
    pub rust_parameters: Vec<RustParameter>,
    pub c_parameters: Vec<CParameter>,
    pub transformations: Vec<Transformation>,
}

impl Parameters {
    fn new(capacity: usize) -> Parameters {
        Parameters {
            rust_parameters: Vec::with_capacity(capacity),
            c_parameters: Vec::with_capacity(capacity),
            transformations: Vec::with_capacity(capacity),
        }
    }

    pub fn analyze_return(&mut self, env: &Env, ret: &Option<analysis::Parameter>) {
        let ret_data = ret
            .as_ref()
            .map(|r| (r.lib_par.array_length, &r.try_from_glib));

        let (ind_c, try_from_glib) = match ret_data {
            Some((Some(array_length), try_from_glib)) => (array_length as usize, try_from_glib),
            _ => return,
        };

        let c_par = if let Some(c_par) = self.c_parameters.get_mut(ind_c) {
            c_par.try_from_glib = try_from_glib.clone();
            c_par
        } else {
            return;
        };

        let transformation = Transformation {
            ind_c,
            ind_rust: None,
            transformation_type: get_length_type(env, "", &c_par.name, c_par.typ),
        };
        self.transformations.push(transformation);
    }
}

#[allow(clippy::useless_let_if_seq)]
pub fn analyze(
    env: &Env,
    function_parameters: &[library::Parameter],
    configured_functions: &[&config::functions::Function],
    disable_length_detect: bool,
    async_func: bool,
    in_trait: bool,
) -> Parameters {
    let mut parameters = Parameters::new(function_parameters.len());

    // Map: length argument position => parameter
    let array_lengths: HashMap<u32, &library::Parameter> = function_parameters
        .iter()
        .filter_map(|p| p.array_length.map(|pos| (pos, p)))
        .collect();

    for (pos, par) in function_parameters.iter().enumerate() {
        let name = if par.instance_parameter {
            par.name.clone()
        } else {
            nameutil::mangle_keywords(&*par.name).into_owned()
        };

        let configured_parameters = configured_functions.matched_parameters(&name);

        let c_type = par.c_type.clone();
        let typ = override_string_type_parameter(env, par.typ, &configured_parameters);

        let ind_c = parameters.c_parameters.len();
        let mut ind_rust = Some(parameters.rust_parameters.len());

        let mut add_rust_parameter = match par.direction {
            library::ParameterDirection::In | library::ParameterDirection::InOut => true,
            library::ParameterDirection::Return => false,
            library::ParameterDirection::Out => !can_as_return(env, par) && !async_func,
            library::ParameterDirection::None => {
                panic!("undefined direction for parameter {:?}", par)
            }
        };

        if async_func && async_param_to_remove(&par.name) {
            add_rust_parameter = false;
        }

        let mut array_par = configured_parameters.iter().find_map(|cp| {
            cp.length_of
                .as_ref()
                .and_then(|n| function_parameters.iter().find(|fp| fp.name == *n))
        });
        if array_par.is_none() {
            array_par = array_lengths.get(&(pos as u32)).copied();
        }
        if array_par.is_none() && !disable_length_detect {
            array_par = detect_length(env, pos, par, function_parameters);
        }
        if let Some(array_par) = array_par {
            let mut array_name = nameutil::mangle_keywords(&array_par.name);
            if let Some(bound_type) = Bounds::type_for(env, array_par.typ) {
                array_name = (array_name.into_owned()
                    + &Bounds::get_to_glib_extra(
                        &bound_type,
                        *array_par.nullable,
                        array_par.instance_parameter,
                    ))
                    .into();
            }

            add_rust_parameter = false;

            let transformation = Transformation {
                ind_c,
                ind_rust: None,
                transformation_type: get_length_type(env, &array_name, &par.name, typ),
            };
            parameters.transformations.push(transformation);
        }

        let mut caller_allocates = par.caller_allocates;
        let mut transfer = par.transfer;
        let conversion = ConversionType::of(env, typ);
        if let ConversionType::Direct
        | ConversionType::Scalar
        | ConversionType::Option
        | ConversionType::Result { .. } = conversion
        {
            //For simple types no reason to have these flags
            caller_allocates = false;
            transfer = library::Transfer::None;
        }

        let immutable = configured_parameters.iter().any(|p| p.constant);
        let ref_mode =
            RefMode::without_unneeded_mut(env, par, immutable, in_trait && par.instance_parameter);

        let nullable_override = configured_parameters.iter().find_map(|p| p.nullable);
        let nullable = nullable_override.unwrap_or(par.nullable);

        let try_from_glib = TryFromGlib::from_parameter(env, typ, &configured_parameters);

        let c_par = CParameter {
            name: name.clone(),
            typ,
            c_type,
            instance_parameter: par.instance_parameter,
            direction: par.direction,
            transfer,
            caller_allocates,
            nullable,
            ref_mode,
            is_error: par.is_error,
            scope: par.scope,
            user_data_index: par.closure,
            destroy_index: par.destroy,
            try_from_glib: try_from_glib.clone(),
        };
        parameters.c_parameters.push(c_par);

        let data_param_name = "user_data";
        let callback_param_name = "callback";

        if add_rust_parameter {
            let rust_par = RustParameter {
                name: name.clone(),
                typ,
                ind_c,
                allow_none: par.allow_none,
            };
            parameters.rust_parameters.push(rust_par);
        } else {
            ind_rust = None;
        }

        let transformation_type = match conversion {
            ConversionType::Direct => {
                if par.c_type != "GLib.Pid" {
                    TransformationType::ToGlibDirect { name }
                } else {
                    TransformationType::ToGlibScalar {
                        name,
                        nullable,
                        needs_into: false,
                    }
                }
            }
            ConversionType::Scalar => TransformationType::ToGlibScalar {
                name,
                nullable,
                needs_into: false,
            },
            ConversionType::Option => {
                let needs_into = match try_from_glib {
                    TryFromGlib::Option => par.direction == library::ParameterDirection::In,
                    TryFromGlib::OptionMandatory => false,
                    other => unreachable!("{:?} inconsistent / conversion type", other),
                };
                TransformationType::ToGlibScalar {
                    name,
                    nullable: Nullable(false),
                    needs_into,
                }
            }
            ConversionType::Result { .. } => {
                let needs_into = match try_from_glib {
                    TryFromGlib::Result { .. } => par.direction == library::ParameterDirection::In,
                    TryFromGlib::ResultInfallible { .. } => false,
                    other => unreachable!("{:?} inconsistent / conversion type", other),
                };
                TransformationType::ToGlibScalar {
                    name,
                    nullable: Nullable(false),
                    needs_into,
                }
            }
            ConversionType::Pointer => TransformationType::ToGlibPointer {
                name,
                instance_parameter: par.instance_parameter,
                transfer,
                ref_mode,
                to_glib_extra: Default::default(),
                explicit_target_type: Default::default(),
                pointer_cast: Default::default(),
                in_trait,
                nullable: *nullable,
            },
            ConversionType::Borrow => TransformationType::ToGlibBorrow,
            ConversionType::Unknown => TransformationType::ToGlibUnknown { name },
        };

        let mut transformation = Transformation {
            ind_c,
            ind_rust,
            transformation_type,
        };
        let mut transformation_type = None;
        match transformation.transformation_type {
            TransformationType::ToGlibDirect { ref name, .. }
            | TransformationType::ToGlibUnknown { ref name, .. } => {
                if async_func && name == callback_param_name {
                    // Remove the conversion of callback for async functions.
                    transformation_type = Some(TransformationType::ToSome(name.clone()));
                }
            }
            TransformationType::ToGlibPointer { ref name, .. } => {
                if async_func && name == data_param_name {
                    // Do the conversion of user_data for async functions.
                    // In async functions, this argument is used to send the callback.
                    transformation_type = Some(TransformationType::IntoRaw(name.clone()));
                }
            }
            _ => (),
        }
        if let Some(transformation_type) = transformation_type {
            transformation.transformation_type = transformation_type;
        }
        parameters.transformations.push(transformation);
    }

    parameters
}

fn get_length_type(
    env: &Env,
    array_name: &str,
    length_name: &str,
    length_typ: TypeId,
) -> TransformationType {
    let array_length_type = RustType::try_new(env, length_typ).into_string();
    TransformationType::Length {
        array_name: array_name.to_string(),
        array_length_name: length_name.to_string(),
        array_length_type,
    }
}

fn detect_length<'a>(
    env: &Env,
    pos: usize,
    par: &library::Parameter,
    parameters: &'a [library::Parameter],
) -> Option<&'a library::Parameter> {
    if !is_length(par) {
        return None;
    }

    parameters.get(pos - 1).and_then(|p| {
        if has_length(env, p.typ) {
            Some(p)
        } else {
            None
        }
    })
}

fn is_length(par: &library::Parameter) -> bool {
    if par.direction != library::ParameterDirection::In {
        return false;
    }

    let len = par.name.len();
    if len >= 3 && &par.name[len - 3..len] == "len" {
        return true;
    }

    par.name.contains("length")
}

fn has_length(env: &Env, typ: TypeId) -> bool {
    use crate::library::Basic::*;
    use crate::library::Type;
    let typ = env.library.type_(typ);
    match typ {
        Type::Basic(Utf8 | Filename | OsString) => true,
        Type::CArray(..)
        | Type::FixedArray(..)
        | Type::Array(..)
        | Type::PtrArray(..)
        | Type::List(..)
        | Type::SList(..)
        | Type::HashTable(..) => true,
        Type::Alias(alias) => has_length(env, alias.typ),
        _ => false,
    }
}

pub fn async_param_to_remove(name: &str) -> bool {
    name == "user_data" || name.ends_with("data") // FIXME: use async indexes instead
}
