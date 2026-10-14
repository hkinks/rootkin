//! Rootkin command-line tool for `.familyx` files.

mod example;

use anyhow::{bail, Context, Result};
use familyx::{codec, model::Tree, Package};

const USAGE: &str = "usage:
  rootkin-cli dump <file.familyx> [out.xml]   write decoded tree XML (stdout if no out)
  rootkin-cli pack <file.familyx> <in.xml> <out.familyx>
                                          replace the tree XML and save as a new file
  rootkin-cli roundtrip <file.familyx>        verify decode/encode is lossless
  rootkin-cli summary <file.familyx>          list people
  rootkin-cli check <file.familyx>            report dangling references
  rootkin-cli example <out.familyx>           write a fictional example family
  rootkin-cli import <in.ged|in.gramps> <out.familyx>
                                          convert a GEDCOM / Gramps file, print what was dropped
  rootkin-cli export <tree.familyx> <out.ged|out.gramps>
                                          write a GEDCOM 5.5.1 / Gramps XML file
  rootkin-cli save12 <tree.familyx> <out.familyx>
                                          write a copy My Family Tree 12.x can open";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["dump", file] => {
            let pkg = Package::open(file).context("open")?;
            std::io::Write::write_all(&mut std::io::stdout(), &pkg.doc.to_bytes())?;
        }
        ["dump", file, out] => std::fs::write(out, Package::open(file).context("open")?.doc.to_bytes())?,
        ["pack", file, xml, out] => {
            let mut pkg = Package::open(file).context("open")?;
            pkg.doc = familyx::doc::Document::parse(&std::fs::read(xml)?)?;
            pkg.save(out)?;
        }
        ["roundtrip", file] => {
            let pkg = Package::open(file).context("open")?;
            let xml = pkg.doc.to_bytes();
            let again = Package::from_bytes(&pkg.to_bytes()?)?;
            if again.doc.to_bytes() != xml {
                bail!("round-trip changed the XML");
            }
            let blob = codec::encode(&xml);
            println!("round-trip OK: {} bytes XML, {} bytes encrypted", xml.len(), blob.len());
        }
        ["summary", file] => {
            let pkg = Package::open(file).context("open")?;
            let tree = Tree::from_doc(&pkg.doc);
            for p in &tree.people {
                println!("{}  {}  {}", p.id, p.display_name(), p.lifespan());
            }
            println!(
                "{} people, {} couples, {} parent links, {} media",
                tree.people.len(),
                tree.spouse_links.len(),
                tree.parent_links.len(),
                tree.media.len()
            );
        }
        ["example", out] => {
            let pkg = example::build()?;
            let problems = familyx::edit::check(&pkg.doc);
            if !problems.is_empty() {
                bail!("generated tree is inconsistent: {problems:?}");
            }
            std::fs::write(out, pkg.to_bytes()?)?;
        }
        ["import", file, out] => {
            let (pkg, report) = rootkin_formats::import_path(file).context("import")?;
            pkg.save(out)?;
            println!("imported {} people, {} families", report.people, report.families);
            for w in &report.warnings {
                println!("warning: {w}");
            }
        }
        ["export", file, out] => {
            let pkg = Package::open(file).context("open")?;
            rootkin_formats::export_path(&pkg, out).context("export")?;
        }
        ["save12", file, out] => {
            let downgrade = Package::open(file).context("open")?.save_for_app12(out)?;
            if downgrade.password_removed {
                println!("note: the copy has no password (version 12 can't read the current password lock)");
            }
            if downgrade.task_places > 0 {
                println!("note: {} task place(s) reduced to their description", downgrade.task_places);
            }
        }
        ["check", file] => {
            let problems = familyx::edit::check(&Package::open(file).context("open")?.doc);
            for p in &problems {
                println!("{p}");
            }
            if !problems.is_empty() {
                bail!("{} problem(s)", problems.len());
            }
            println!("OK");
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
    Ok(())
}
