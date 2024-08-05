use std::{env, fmt, fs, path::PathBuf, process::exit};

use chrono::NaiveDate;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Debug, Default, EnumIter)]
enum Category {
    Grocery,
    Entrateinment,
    Restaurants,
    Shopping,
    Transport,
    Travel,
    Miscellaneous(String),
    #[default]
    Unknown
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
        for c in Category::iter()  {
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
            },
            _ => {},
        }
    };

    return path;
}

fn parse_file(filepath: PathBuf) -> Vec<Entry> {
    let content = fs::read_to_string(&filepath).unwrap_or_default();
    let lines = content.lines().skip(1);

    let mut entries = vec![];

    for (i, line) in lines.enumerate() {
        let fields = line.split(';');
        let mut entry = Entry::default();
        for (i, field) in fields.enumerate() {
            match i {
                0 => {
                    let mut parts = field.split('.');
                    let units = parts.next().unwrap().parse::<i32>().unwrap();
                    let cents = parts.next().unwrap_or("0").parse::<u32>().unwrap_or(0);
                    entry.value = (units, cents);
                },
                1 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field, "%d/%m/%Y") {
                        entry.date = date;
                    } else {
                        eprintln!("[ERROR] Could not parse date `{}` in {}:{}", field, filepath.display(), i+2);
                        exit(1);
                    }
                },
                2 => {
                    entry.category = Category::from(field);
                },
                3 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field, "%d/%m/%Y") {
                        entry.end_date = date;
                    } else {
                        eprintln!("[ERROR] Could not parse date `{}` in {}:{}", field, filepath.display(), i+2);
                        exit(1);
                    }
                },
                4 => {
                    entry.note = String::from(field);
                },
                _ => {}
            }
        }
        entries.push(entry);
    }

    return entries;
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
}
