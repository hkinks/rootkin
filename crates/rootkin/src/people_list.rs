//! The people list on the left: search, sort, surname letters and a
//! virtualized list that stays fast for tens of thousands of people.

use eframe::egui::{self, RichText};
use familyx::model::{Person, Tree};

use crate::app::{sort_key, App};

/// From this many people on, sorting by surname adds letter headings and an A-Z strip.
pub(crate) const LETTERS_FROM: usize = 200;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum SortBy {
    Surname,
    GivenName,
    Birth,
}

/// One line of the list.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Row {
    Letter(char),
    /// Index into `Tree::people`.
    Person(usize),
}

/// What the list was built from; when it changes the list is rebuilt.
#[derive(Clone, PartialEq)]
struct Key {
    epoch: u64,
    search: String,
    sort_by: SortBy,
    living_only: bool,
}

#[derive(Default)]
pub(crate) struct ListCache {
    key: Option<Key>,
    /// Number of people shown.
    shown: usize,
    rows: Vec<Row>,
    /// Each letter present and the row where it starts.
    letters: Vec<(char, usize)>,
}

fn birth_year(p: &Person) -> i32 {
    p.fact("BIRT").and_then(|f| f.date.year).unwrap_or(i32::MAX)
}

/// The heading letter for a surname: its first letter in capitals, or `#`.
fn letter_of(p: &Person) -> char {
    p.primary_name()
        .and_then(|n| n.surname.chars().next())
        .filter(|c| c.is_alphabetic())
        .map_or('#', |c| c.to_uppercase().next().unwrap_or(c))
}

/// Indices into `tree.people` that match, in display order. Within the same
/// name the older person comes first.
pub(crate) fn ordered(tree: &Tree, search: &str, sort_by: SortBy, living_only: bool) -> Vec<usize> {
    let needle = search.to_lowercase();
    let matches = |p: &Person| {
        needle.is_empty()
            || p.names.iter().any(|n| n.display().to_lowercase().contains(&needle))
            || p.facts.iter().any(|f| f.place.to_lowercase().contains(&needle) || f.date.display().contains(&needle))
    };
    let mut order: Vec<usize> = tree
        .people
        .iter()
        .enumerate()
        .filter(|(_, p)| (!living_only || p.is_living) && matches(p))
        .map(|(i, _)| i)
        .collect();
    let people = &tree.people;
    match sort_by {
        SortBy::Surname => order.sort_by_cached_key(|&i| {
            let (unnamed, surname, given) = sort_key(&people[i]);
            (unnamed, surname, given, birth_year(&people[i]))
        }),
        SortBy::GivenName => order.sort_by_cached_key(|&i| {
            let (unnamed, surname, given) = sort_key(&people[i]);
            (unnamed, given, surname, birth_year(&people[i]))
        }),
        SortBy::Birth => order.sort_by_cached_key(|&i| (birth_year(&people[i]), sort_key(&people[i]))),
    }
    order
}

/// The list lines for `order`, with a letter heading before each new
/// surname initial when `letters` is on. Also where each letter starts.
pub(crate) fn build_rows(tree: &Tree, order: &[usize], letters: bool) -> (Vec<Row>, Vec<(char, usize)>) {
    let mut rows = Vec::with_capacity(order.len() + 30);
    let mut starts = Vec::new();
    let mut last = None;
    for &i in order {
        if letters {
            let letter = letter_of(&tree.people[i]);
            if last != Some(letter) {
                starts.push((letter, rows.len()));
                rows.push(Row::Letter(letter));
                last = Some(letter);
            }
        }
        rows.push(Row::Person(i));
    }
    (rows, starts)
}

impl App {
    fn refresh_list(&mut self) {
        let key = Key {
            epoch: self.tree_epoch,
            search: self.search.clone(),
            sort_by: self.sort_by,
            living_only: self.living_only,
        };
        if self.list_cache.key.as_ref() == Some(&key) {
            return;
        }
        let order = ordered(&self.tree, &key.search, key.sort_by, key.living_only);
        let letters = key.sort_by == SortBy::Surname && order.len() > LETTERS_FROM;
        let (rows, starts) = build_rows(&self.tree, &order, letters);
        self.list_cache = ListCache { key: Some(key), shown: order.len(), rows, letters: starts };
    }

    pub(crate) fn people_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search names, places, years…")
                    .desired_width(ui.available_width() - 28.0),
            );
            if !self.search.is_empty() && ui.small_button("✖").on_hover_text("Clear search").clicked() {
                self.search.clear();
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("sort")
                .selected_text(match self.sort_by {
                    SortBy::Surname => "Sort: surname",
                    SortBy::GivenName => "Sort: first name",
                    SortBy::Birth => "Sort: birth year",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort_by, SortBy::Surname, "Surname");
                    ui.selectable_value(&mut self.sort_by, SortBy::GivenName, "First name");
                    ui.selectable_value(&mut self.sort_by, SortBy::Birth, "Birth year");
                });
            ui.checkbox(&mut self.living_only, "Living only");
        });

        self.refresh_list();
        ui.weak(format!("{} of {} people", self.list_cache.shown, self.tree.people.len()));

        let row_height = ui
            .spacing()
            .interact_size
            .y
            .max(ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().button_padding.y);
        if !self.list_cache.letters.is_empty() {
            let mut jump = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &(letter, row) in &self.list_cache.letters {
                    if ui.small_button(letter.to_string()).on_hover_text("Jump to this letter").clicked() {
                        jump = Some(row);
                    }
                }
            });
            if jump.is_some() {
                self.list_jump = jump;
            }
        }
        ui.separator();

        let mut area = egui::ScrollArea::vertical().auto_shrink(false);
        if let Some(row) = self.list_jump.take() {
            area = area.vertical_scroll_offset(row as f32 * (row_height + ui.spacing().item_spacing.y));
        }
        let mut clicked = None;
        let rows = &self.list_cache.rows;
        area.show_rows(ui, row_height, rows.len(), |ui, range| {
            for row in &rows[range] {
                match *row {
                    Row::Letter(letter) => {
                        ui.add_sized(
                            [ui.available_width(), row_height],
                            egui::Label::new(RichText::new(letter.to_string()).strong()).selectable(false),
                        );
                    }
                    Row::Person(i) => {
                        let p = &self.tree.people[i];
                        let is_selected = self.selected.as_deref() == Some(p.id.as_str());
                        let label = format!("{}  {}", p.display_name(), p.lifespan());
                        if ui.selectable_label(is_selected, label).clicked() {
                            clicked = Some(p.id.clone());
                        }
                    }
                }
            }
        });
        if let Some(id) = clicked {
            self.select(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use familyx::edit::{self, FactInput};
    use familyx::model::Gender;
    use familyx::Package;

    fn tree_of(people: &[(&str, &str, Option<i32>)]) -> Tree {
        let mut pkg = Package::new();
        for (given, surname, born) in people {
            let id = edit::add_person(&mut pkg.doc, given, surname, Gender::Unknown);
            if let Some(year) = born {
                let input = FactInput { date: familyx::date::Date::year(*year), ..Default::default() };
                edit::add_fact(&mut pkg.doc, &id, "BIRT", &input).unwrap();
            }
        }
        Tree::from_doc(&pkg.doc)
    }

    fn names(tree: &Tree, order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| format!("{} {:?}", tree.people[i].display_name(), birth_year(&tree.people[i]))).collect()
    }

    #[test]
    fn same_name_is_sorted_by_birth_year() {
        let tree = tree_of(&[
            ("Aleksander", "Ilves", Some(1901)),
            ("Aleksander", "Ilves", Some(1850)),
            ("Aleksander", "Ilves", None),
            ("Aleksander", "Ilves", Some(1875)),
            ("Mari", "Ilves", Some(1800)),
        ]);
        for sort in [SortBy::Surname, SortBy::GivenName] {
            let order = ordered(&tree, "", sort, false);
            let years: Vec<i32> = order
                .iter()
                .filter(|&&i| tree.people[i].display_name() == "Aleksander Ilves")
                .map(|&i| birth_year(&tree.people[i]))
                .collect();
            assert_eq!(years, vec![1850, 1875, 1901, i32::MAX], "{:?}", names(&tree, &order));
        }
    }

    #[test]
    fn letters_group_surnames_and_record_where_they_start() {
        let tree = tree_of(&[("A", "Aas", None), ("B", "Aru", None), ("C", "Mägi", None), ("D", "", None)]);
        let order = ordered(&tree, "", SortBy::Surname, false);
        let (rows, starts) = build_rows(&tree, &order, true);
        assert_eq!(starts, vec![('#', 0), ('A', 2), ('M', 5)]);
        assert_eq!(rows[0], Row::Letter('#'));
        assert_eq!(rows.len(), 4 + 3);
        let (plain, none) = build_rows(&tree, &order, false);
        assert_eq!(plain.len(), 4);
        assert!(none.is_empty());
    }

    #[test]
    fn twenty_thousand_people_are_ordered_quickly() {
        let people: Vec<(String, String, Option<i32>)> =
            (0..20_000).map(|i| (format!("P{i}"), format!("S{}", i % 700), Some(1800 + i % 150))).collect();
        let refs: Vec<(&str, &str, Option<i32>)> = people.iter().map(|(g, s, y)| (&g[..], &s[..], *y)).collect();
        let tree = tree_of(&refs);
        let start = std::time::Instant::now();
        let order = ordered(&tree, "", SortBy::Surname, false);
        let (rows, _) = build_rows(&tree, &order, true);
        let found = ordered(&tree, "s17", SortBy::Surname, false);
        assert_eq!((order.len(), rows.len() > 20_000, found.len() > 20), (20_000, true, true));
        assert!(start.elapsed().as_secs_f32() < 5.0, "took {:?}", start.elapsed());
    }
}
