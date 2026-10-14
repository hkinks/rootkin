//! Import time must grow roughly in step with the tree. A generated, fictional
//! pedigree of the size an online tree export can reach.

use std::fmt::Write;
use std::time::Instant;

use rootkin_formats::{import, Format};

/// `n` people in couples with 1 to 3 children each, every person with birth
/// (cited) and some with death, plus notes.
fn generate(n: usize) -> String {
    let mut out = String::from("0 HEAD\n1 CHAR UTF-8\n0 @S1@ SOUR\n1 TITL Parish register\n");
    let mut fams = String::new();
    let (mut next, mut fam_no) = (1usize, 0usize);
    // people waiting to become parents: (husband, wife)
    let mut queue = std::collections::VecDeque::new();
    let person = |out: &mut String, i: usize| {
        let sex = if i.is_multiple_of(2) { "M" } else { "F" };
        writeln!(out, "0 @I{i}@ INDI\n1 NAME Given{i} /Surname{}/\n1 SEX {sex}", i % 97).unwrap();
        writeln!(
            out,
            "1 BIRT\n2 DATE {} MAR {}\n2 PLAC Town {}, Region\n2 SOUR @S1@\n3 PAGE p. {i}",
            1 + i % 28,
            1700 + i % 250,
            i % 40
        )
        .unwrap();
        if i.is_multiple_of(3) {
            writeln!(out, "1 DEAT\n2 DATE {}\n1 NOTE Remembered in the village.", 1760 + i % 240).unwrap();
        }
    };
    // a founding couple
    for _ in 0..2 {
        person(&mut out, next);
        next += 1;
    }
    queue.push_back((1, 2));
    while next <= n {
        let Some((h, w)) = queue.pop_front() else { break };
        fam_no += 1;
        writeln!(fams, "0 @F{fam_no}@ FAM\n1 HUSB @I{h}@\n1 WIFE @I{w}@\n1 MARR\n2 DATE {}", 1720 + fam_no % 230)
            .unwrap();
        let kids = 1 + fam_no % 3;
        let mut grown = Vec::new();
        for _ in 0..kids {
            if next > n {
                break;
            }
            person(&mut out, next);
            writeln!(fams, "1 CHIL @I{next}@").unwrap();
            grown.push(next);
            next += 1;
        }
        // each child marries a newcomer, who is added when the family is formed
        for a in grown {
            if next <= n {
                person(&mut out, next);
                queue.push_back(if a.is_multiple_of(2) { (a, next) } else { (next, a) });
                next += 1;
            }
        }
    }
    out.push_str(&fams);
    out.push_str("0 TRLR\n");
    out
}

fn timed(n: usize) -> f64 {
    let ged = generate(n);
    let start = Instant::now();
    let (pkg, report) = import(ged.as_bytes(), Format::Gedcom).unwrap();
    let secs = start.elapsed().as_secs_f64();
    eprintln!("import of {} people, {} families: {secs:.2} s", report.people, report.families);
    assert!(report.people >= n);
    assert!(familyx::edit::check(&pkg.doc).is_empty());
    secs
}

#[test]
fn import_time_grows_roughly_linearly() {
    let small = timed(1_000);
    let large = timed(5_000);
    // five times the people: linear is ~5x; the old quadratic code took ~25x
    assert!(large < small * 12.0 + 1.0, "1000 people took {small:.2} s but 5000 took {large:.2} s");
    assert!(large < 60.0, "5000 people took {large:.2} s");
}

#[test]
#[ignore = "slow; run with --release --ignored --nocapture"]
fn import_20000_people() {
    timed(20_000);
}
