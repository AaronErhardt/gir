//use std::collections::HashMap;
use std::ops::Index;

use library;
use nameutil;
use version::Version;

pub type NsId = u16;
pub const MAIN: NsId = library::MAIN_NAMESPACE;

#[derive(Debug)]
pub struct Namespace {
    pub name: String,
    pub crate_name: String,
    pub package_name: Option<String>,
    pub versions: Vec<Version>,
}

#[derive(Debug)]
pub struct Info {
    namespaces: Vec<Namespace>,
    //name_index: HashMap<String, NsId>,
    pub glib_ns_id: NsId,
}

impl Info {
    /*
    pub fn by_name(&self, name: &str) -> Option<NsId> {
        self.name_index.get(name).cloned()
    }
    */

    pub fn main(&self) -> &Namespace {
        &self[MAIN]
    }

    pub fn len(&self) -> usize {
        self.namespaces.len()
    }
}

impl Index<NsId> for Info {
    type Output = Namespace;

    fn index(&self, index: NsId) -> &Namespace {
        &self.namespaces[index as usize]
    }
}

pub fn run(gir: &library::Library) -> Info {
    let mut namespaces = Vec::with_capacity(gir.namespaces.len());
    //let mut name_index = HashMap::with_capacity(gir.namespaces.len());
    let mut glib_ns_id = None;

    for (ns_id, ns) in gir.namespaces.iter().enumerate() {
        let ns_id = ns_id as NsId;
        namespaces.push(Namespace {
            name: ns.name.clone(),
            crate_name: nameutil::crate_name(&ns.name),
            package_name: ns.package_name.clone(),
            versions: ns.versions.iter().cloned().collect(),
        });
        //name_index.insert(ns.name.clone(), ns_id);
        if ns.name == "GLib" {
            glib_ns_id = Some(ns_id);
        }
    }

    Info {
        namespaces: namespaces,
        //name_index: name_index,
        glib_ns_id: glib_ns_id.expect("Missing `GLib` namespace!"),
    }
}
