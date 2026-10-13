//! Debugging tool for `.familyx` files.

use anyhow::{bail, Context, Result};
use familyx::{codec, model::Tree, Package};

const USAGE: &str = "usage:
  familyx dump <file.familyx> [out.xml]   write decoded tree XML (stdout if no out)
  familyx pack <file.familyx> <in.xml> <out.familyx>
                                          replace the tree XML and save as a new file
  familyx roundtrip <file.familyx>        verify decode/encode is lossless
  familyx summary <file.familyx>          list people
  familyx check <file.familyx>            report dangling references";

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
