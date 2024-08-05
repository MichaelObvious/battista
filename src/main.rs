use std::{
    cmp::Ordering, collections::HashMap, env, fmt, fs, mem, path::PathBuf, process::exit, vec,
};

use chrono::{Duration, NaiveDate, Utc};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Clone, Debug, Default, EnumIter, PartialEq, Hash, Eq)]
enum Category {
    Grocery,
    Entrateinment,
    Restaurants,
    Shopping,
    Transport,
    Travel,
    Miscellaneous(String),
    #[default]
    Unknown,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", {
            match self {
                Self::Grocery => String::from("Grocery"),
                Self::Entrateinment => String::from("Entrateinment"),
                Self::Restaurants => String::from("Restaurants"),
                Self::Shopping => String::from("Shopping"),
                Self::Transport => String::from("Transport"),
                Self::Travel => String::from("Travel"),
                Self::Miscellaneous(a) => format!("Miscellaneous: {}", a),
                Self::Unknown => String::from("Unknown"),
            }
        })
    }
}

impl From<&str> for Category {
    fn from(s: &str) -> Self {
        for c in Category::iter() {
            if &format!("{}", c) == s {
                return c;
            }
        }
        return Self::Miscellaneous(String::from(s));
    }
}

#[derive(Debug, Default)]
struct Entry {
    value: (i32, u32), // units and cents
    date: NaiveDate,
    category: Category,
    end_date: NaiveDate,
    note: String,
}

struct DateRange(NaiveDate, NaiveDate);

impl Iterator for DateRange {
    type Item = NaiveDate;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 <= self.1 {
            let next = self.0 + Duration::days(1);
            Some(mem::replace(&mut self.0, next))
        } else {
            None
        }
    }
}

fn print_usage() {
    println!("USAGE: {} <path/to/file.csv>", env::args().next().unwrap());
}

fn get_path() -> Option<PathBuf> {
    let args = env::args().skip(1);

    let mut path = None;
    for arg in args {
        let cur_path = PathBuf::from(arg);
        match cur_path.try_exists() {
            Ok(true) => {
                path = Some(cur_path);
                break;
            }
            _ => {}
        }
    }

    return path;
}

fn parse_file(filepath: PathBuf) -> Vec<Entry> {
    let content = fs::read_to_string(&filepath).unwrap_or_default();
    let lines = content.lines().skip(1);

    let mut entries = vec![];

    for (line_idx, line) in lines.enumerate() {
        let fields = line.split(';');
        let mut entry = Entry::default();
        for (field_idx, field) in fields.enumerate() {
            match field_idx {
                0 => {
                    let mut parts = field.split('.');
                    let units = parts.next().unwrap().parse::<i32>().unwrap();
                    let cents = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
                    entry.value = (units, cents);
                }
                1 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field, "%d/%m/%Y") {
                        entry.date = date;
                    } else {
                        eprintln!(
                            "[ERROR] Could not parse date `{}` in {}:{}",
                            field,
                            filepath.display(),
                            line_idx + 2
                        );
                        exit(1);
                    }
                }
                2 => {
                    entry.category = Category::from(field);
                }
                3 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field, "%d/%m/%Y") {
                        entry.end_date = date;
                    } else {
                        eprintln!(
                            "[ERROR] Could not parse date `{}` in {}:{}",
                            field,
                            filepath.display(),
                            line_idx + 2
                        );
                        exit(1);
                    }
                }
                4 => {
                    entry.note = String::from(field);
                }
                _ => {}
            }
        }

        if Ordering::is_gt(entry.date.cmp(&entry.end_date)) {
            eprintln!(
                "[ERROR] Date is later than end date in {}:{}",
                filepath.display(),
                line_idx + 2
            );
            exit(1);
        }

        entries.push(entry);
    }

    entries.sort_by(|a, b| a.date.cmp(&b.date));

    return entries;
}

#[derive(Default, Debug)]
struct Stats {
    average_spending_per_day: Vec<(NaiveDate, f64)>,
    spent_last_month: f64,
    spent_last_month_by_category: Vec<(Category, f64)>,
    spent_last_year: f64,
    spent_last_year_by_category: Vec<(Category, f64)>,
    spent_current_year_by_month: Vec<(NaiveDate, f64)>,
    spent_current_year: f64,
}

fn gather_stats(entries: &Vec<Entry>) -> Stats {
    let today = Utc::now().date_naive();

    let mut days: HashMap<NaiveDate, f64> = DateRange(entries.first().unwrap().date, today)
        .map(|x| (x, 0.0))
        .collect();

    let mut spent_last_month = 0.0;
    let mut category_month_spent = HashMap::new();

    let mut spent_last_year = 0.0;
    let mut category_year_spent = HashMap::new();

    for entry in entries.iter() {
        let num_days = (entry.end_date - entry.date).num_days().max(1);
        let cents = entry.value.1 as f64;
        let value = entry.value.0 as f64 + cents / 10.0_f64.powf((cents + 1.0).log10().ceil());
        let average_value = value / num_days as f64;
        for d in DateRange(entry.date, entry.end_date.min(today)) {
            *days.get_mut(&d).unwrap() += average_value;
        }

        if (entry.date - today).num_days() <= 30 {
            spent_last_month += value;
            let prev = category_month_spent.get(&entry.category).unwrap_or(&0.0);
            category_month_spent.insert(&entry.category, prev + value);
        }

        if (entry.date - today).num_days() <= 365 {
            spent_last_year += value;
            let prev = category_year_spent.get(&entry.category).unwrap_or(&0.0);
            category_year_spent.insert(&entry.category, prev + value);
        }
    }

    let mut average_spending_per_day: Vec<_> = days
        .iter()
        .map(|a| (a.0.to_owned(), a.1.to_owned()))
        .collect();
    average_spending_per_day.sort_by(|a, b| a.0.cmp(&b.0));

    let mut spent_last_month_by_category: Vec<_> = category_month_spent
        .iter()
        .map(|a| ((**a.0).clone(), a.1.to_owned()))
        .collect();
    spent_last_month_by_category.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let mut spent_last_year_by_category: Vec<_> = category_month_spent
        .iter()
        .map(|a| ((**a.0).clone(), a.1.to_owned()))
        .collect();
    spent_last_year_by_category.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    return Stats {
        average_spending_per_day: average_spending_per_day,
        spent_last_month: spent_last_month,
        spent_last_month_by_category: spent_last_month_by_category,
        spent_last_year: spent_last_year,
        spent_last_year_by_category: spent_last_year_by_category,
        spent_current_year_by_month: vec![],
        spent_current_year: 0.0,
    };
}

fn print_stats(stats: &Stats) {
    println!("---");
    let to_skip = (stats.average_spending_per_day.len() as isize - 30).max(0);
    for (date, spent) in stats.average_spending_per_day.iter().skip(to_skip as usize) {
        println!("{}: {:.2}", date, spent);
    }
    println!("---");
    println!(
        "Spent last month: {:.2} ({:.2} per day)",
        stats.spent_last_month,
        stats.spent_last_month / 30.0
    );
    for (category, spent) in stats.spent_last_month_by_category.iter() {
        println!("{}: {:.2}", category, spent);
    }
    println!("---");
    println!(
        "Spent last year: {:.2} ({:.2} per day)",
        stats.spent_last_year,
        stats.spent_last_year / 365.0
    );
    for (category, spent) in stats.spent_last_year_by_category.iter() {
        println!("{}: {:.2}", category, spent);
    }
}

fn main() {
    let path = get_path();

    if path.is_none() {
        eprintln!("[ERROR] No file provided.");
        print_usage();
        return;
    }

    assert!(path.is_some(), "Rust has a problem here.");
    let entries = parse_file(path.unwrap());
    println!("{:?}", entries);

    if entries.is_empty() {
        println!("[INFO] Provided file has no entries. Exiting...");
        return;
    }

    let stats = gather_stats(&entries);
    print_stats(&stats);
}
