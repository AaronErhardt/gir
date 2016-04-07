use std::io::{Result, Write};

use env::Env;
use analysis::bounds::{Bounds, BoundType};
use analysis::parameter::Parameter;
use analysis::ref_mode::RefMode;
use analysis::rust_type::parameter_rust_type;
use analysis::trampolines::Trampoline;
use nameutil;
use super::return_value::ToReturnValue;
use super::sys::ffi_type::ffi_type;
use super::trampoline_from_glib::TrampolineFromGlib;
use super::trampoline_to_glib::TrampolineToGlib;
use traits::IntoString;

pub fn generate(w: &mut Write, env: &Env, analysis: &Trampoline,
                in_trait: bool, object_name: &str) -> Result<()> {
    try!(writeln!(w, ""));
    let (bounds, end) = if in_trait {
        ("<T>", "")
    } else {
        ("", " {")
    };

    let params_str = trampoline_parameters(env, analysis);
    let func_str = func_string(env, analysis);
    let ret_str = trampoline_returns(env, analysis);

    //TODO: version, cfg_condition
    try!(writeln!(w, "unsafe extern \"C\" fn {}{}({}, f: &Box<{}>){}{}",
                  analysis.name, bounds, params_str, func_str, ret_str, end));
    if in_trait {
        try!(writeln!(w, "where T: IsA<{}> {{", object_name));
    }
    try!(writeln!(w, "\tcallback_guard!();"));
    let call = trampoline_call_func(env, analysis, in_trait);
    try!(writeln!(w, "\t{}", call));
    try!(writeln!(w, "}}"));

    Ok(())
}

fn func_string(env: &Env, analysis: &Trampoline) -> String {
    let param_str = func_parameters(env, analysis);
    let return_str = func_returns(env, analysis);

    format!("Fn({}){} + 'static", param_str, return_str)
}

fn func_parameters(env: &Env, analysis: &Trampoline) -> String {
    let mut param_str = String::with_capacity(100);

    for (pos, par) in analysis.parameters.iter().enumerate() {
        if pos > 0 { param_str.push_str(", ") }
        let s = func_parameter(env, par, &analysis.bounds);
        param_str.push_str(&s);
    }

    param_str
}

fn func_parameter(env: &Env, par: &Parameter, bounds: &Bounds) -> String {
    let mut_str = if par.ref_mode == RefMode::ByRefMut { "mut " } else { "" };

    let type_str: String;
    match bounds.get_parameter_alias_info(&par.name) {
        Some((t, bound_type)) => {
            match bound_type {
                BoundType::IsA => if *par.nullable {
                    type_str = format!("Option<&{}{}>", mut_str, t)
                } else {
                    type_str = format!("&{}{}", mut_str, t)
                },
                BoundType::AsRef  => type_str = t.to_owned(),
            }
        }
        None => {
            let rust_type = parameter_rust_type(env, par.typ, par.direction,
                                                par.nullable, par.ref_mode);
            type_str = rust_type.into_string();
        }
    }
    type_str
}

fn func_returns(env: &Env, analysis: &Trampoline) -> String {
    if analysis.ret.typ == Default::default() {
        String::new()
    } else {
        analysis.ret.to_return_value(&env)
    }
}

fn trampoline_parameters(env: &Env, analysis: &Trampoline) -> String {
    let mut parameter_strs: Vec<String> = Vec::new();
    for par in &analysis.parameters {
        let par_str = trampoline_parameter(env, par);
        parameter_strs.push(par_str);
    }

    parameter_strs.join(", ")
}

fn trampoline_parameter(env: &Env, par: &Parameter) -> String {
    let ffi_type = ffi_type(env, par.typ, &par.c_type);
    format!("{}: {}", nameutil::mangle_keywords(&*par.name), ffi_type.into_string())
}

fn trampoline_returns(env: &Env, analysis: &Trampoline) -> String {
    if analysis.ret.typ == Default::default() {
        String::new()
    } else {
        let ffi_type = ffi_type(env, analysis.ret.typ, &analysis.ret.c_type);
        format!(" -> {}", ffi_type.into_string())
    }
}

fn trampoline_call_func(env: &Env, analysis: &Trampoline, in_trait: bool) -> String {
    let params = trampoline_call_parameters(env, analysis, in_trait);
    let ret = if analysis.ret.typ == Default::default() {
        String::new()
    } else {
        analysis.ret.trampoline_to_glib(&env.library)
    };
    format!("f({}){}", params, ret)
}

fn trampoline_call_parameters(env: &Env, analysis: &Trampoline, in_trait: bool) -> String {
    let mut need_downcast = in_trait;
    let mut parameter_strs: Vec<String> = Vec::new();
    for par in &analysis.parameters {
        let par_str = par.trampoline_from_glib(env, need_downcast);
        parameter_strs.push(par_str);
        need_downcast = false;  //Only downcast first parameter
    }

    parameter_strs.join(", ")
}
