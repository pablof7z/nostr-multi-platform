use crate::manifest_edit;
use nmp_codegen::{AppManifest, NmpDependency};

pub fn run(args: &[String]) -> Result<(), String> {
    let manifest = manifest_edit::manifest_arg(args)?;
    let body = manifest_edit::read(&manifest)?;
    let parsed = AppManifest::parse(&body)?;

    println!("manifest: {}", manifest.display());
    println!("app: {}", parsed.name);
    match &parsed.nmp {
        NmpDependency::Version { version } => {
            println!("nmp dependency mode: version");
            println!("nmp version: {version}");
        }
        NmpDependency::Path { path } => {
            println!("nmp dependency mode: path");
            println!("nmp path: {path}");
        }
    }
    println!("modules: {}", parsed.ordered_modules().len());
    Ok(())
}
