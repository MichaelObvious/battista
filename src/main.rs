use std::{
    cmp::Ordering,
    collections::HashMap,
    env,
    fmt::{self, Debug},
    fs,
    hash::Hash,
    mem,
    path::PathBuf,
    process::exit,
    vec,
};

use chrono::{Datelike, Duration, NaiveDate, Utc};
use plotters::{
    chart::ChartBuilder,
    prelude::{BitMapBackend, IntoDrawingArea, IntoLinspace, Rectangle, Text},
    series::LineSeries,
    style::{full_palette::AMBER, Color, FontStyle, IntoFont, RED, WHITE},
};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Clone, Debug, Default, EnumIter, PartialEq, Hash, Eq)]
enum Category {
    Charity,
    Grocery,
    Education,
    Entrateinment,
    Healthcare,
    Hobby,
    Rent,
    Restaurants,
    Savings,
    Shopping,
    Taxes,
    Transportation,
    Travel,
    Utilities,
    Miscellaneous(String),
    #[default]
    Unknown,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", {
            match self {
                Self::Charity => String::from("Charity"),
                Self::Grocery => String::from("Grocery"),
                Self::Education => String::from("Education"),
                Self::Entrateinment => String::from("Entrateinment"),
                Self::Healthcare => String::from("Healthcare"),
                Self::Hobby => String::from("Hobby"),
                Self::Rent => String::from("Rent"),
                Self::Restaurants => String::from("Restaurants"),
                Self::Savings => String::from("Savings"),
                Self::Shopping => String::from("Shopping"),
                Self::Taxes => String::from("Taxes"),
                Self::Transportation => String::from("Transportation"),
                Self::Travel => String::from("Travel"),
                Self::Utilities => String::from("Utilities"),
                Self::Miscellaneous(a) => format!("Miscellaneous ({})", a),
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
    payment_method: String,
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

fn moving_average(xs: Vec<f64>, window: isize) -> Vec<f64> {
    let mut average = Vec::new();

    for i in 0..xs.len() {
        let mut a = 0.0;
        let start = (i as isize - window + 1).max(0) as usize;
        let n = (i - start + 1) as f64;
        for j in start..=i {
            a += xs[j];
        }
        a /= n;
        average.push(a);
    }
    assert!(average.len() == xs.len());
    return average;
}

// fn days_in_month(d: NaiveDate) -> i64 {
//     let year = d.year_ce().1 as i32 * if d.year_ce().0 { 1 } else { -1 };
//     let month = d.month0() + 1;
//     (NaiveDate::from_ymd_opt(year + if month == 12 { 1 } else { 0 }, (month % 12) + 1, 1).unwrap()
//         - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
//     .num_days()
// }

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

fn parse_file(filepath: &PathBuf) -> Vec<Entry> {
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
                    let units = parts.next().unwrap().trim().parse::<i32>().unwrap();
                    let cents = parts
                        .next()
                        .unwrap_or("0")
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(0);
                    entry.value = (units, cents);
                }
                1 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field.trim(), "%d/%m/%Y") {
                        entry.date = date;
                    } else {
                        eprintln!(
                            "[ERROR] Could not parse date `{}` in {}:{}",
                            field.trim(),
                            filepath.display(),
                            line_idx + 2
                        );
                        exit(1);
                    }
                }
                2 => {
                    entry.category = Category::from(field.trim());
                }
                3 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field.trim(), "%d/%m/%Y") {
                        entry.end_date = date;
                    } else {
                        eprintln!(
                            "[ERROR] Could not parse date `{}` in {}:{}",
                            field.trim(),
                            filepath.display(),
                            line_idx + 2
                        );
                        exit(1);
                    }
                }
                4 => {
                    entry.payment_method = String::from(field.trim());
                }
                5 => {
                    entry.note = String::from(field.trim());
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
    spent_current_year: f64,
    spent_current_year_by_category: Vec<(Category, f64, f64)>,
    spent_current_year_by_payment_method: Vec<(String, f64)>,
    spent_current_year_by_month: Vec<(u32, f64, f64)>,
    spent_current_year_per_day: f64,
}

fn gather_stats(entries: &Vec<Entry>) -> Stats {
    let today = Utc::now().date_naive();
    let this_year = today.year_ce().1 as i32 * if today.year_ce().0 { 1 } else { -1 };

    let mut days: HashMap<NaiveDate, f64> = DateRange(entries.first().unwrap().date, today)
        .map(|x| (x, 0.0))
        .collect();

    let mut spent_last_month = 0.0;
    let mut category_month_spent = HashMap::new();

    let mut spent_last_year = 0.0;
    let mut category_year_spent = HashMap::new();

    let mut spent_current_year = 0.0;
    let mut cur_year_month_spent = HashMap::new();
    for i in 1..(today.month0() + 2) {
        cur_year_month_spent.insert(i, 0.0);
    }
    let mut cur_year_category_spent = HashMap::new();
    let mut cur_year_pm_spent = HashMap::new();

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

        if entry.date.year() == this_year {
            spent_current_year += value;

            let entry_month = entry.date.month0() + 1;
            let prev = cur_year_month_spent.get(&entry_month).unwrap_or(&0.0);
            cur_year_month_spent.insert(entry_month, prev + value);

            let prev = cur_year_category_spent.get(&entry.category).unwrap_or(&0.0);
            cur_year_category_spent.insert(&entry.category, prev + value);

            let prev = cur_year_pm_spent.get(&entry.payment_method).unwrap_or(&0.0);
            cur_year_pm_spent.insert(&entry.payment_method, prev + value);
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
    spent_last_month_by_category.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().reverse());

    let mut spent_last_year_by_category: Vec<_> = category_month_spent
        .iter()
        .map(|a| ((**a.0).clone(), a.1.to_owned()))
        .collect();
    spent_last_year_by_category.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().reverse());

    let spent_current_year_per_day = spent_current_year
        / (today - NaiveDate::from_ymd_opt(this_year, 1, 1).unwrap() + Duration::days(1)).num_days()
            as f64;

    let mut spent_current_year_by_category: Vec<_> = cur_year_category_spent
        .iter()
        .map(|a| ((**a.0).clone(), a.1.to_owned()))
        .map(|a| (a.0, a.1, a.1 / spent_current_year))
        .collect();
    spent_current_year_by_category.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().reverse());

    let mut spent_current_year_by_month: Vec<_> = cur_year_month_spent
        .iter()
        .map(|a| (a.0.to_owned(), a.1.to_owned()))
        .map(|x| {
            (
                x.0,
                x.1,
                x.1 / (NaiveDate::from_ymd_opt(
                    this_year + if x.0 == 12 { 1 } else { 0 },
                    (x.0 % 12) + 1,
                    1,
                )
                .unwrap()
                .min(today)
                    - NaiveDate::from_ymd_opt(this_year, x.0, 1).unwrap())
                .num_days() as f64,
            )
        })
        .collect();
    spent_current_year_by_month.sort_by(|a, b| a.0.cmp(&b.0));

    let mut spent_current_year_by_payment_method: Vec<_> = cur_year_pm_spent
        .iter()
        .map(|a| ((**a.0).clone(), a.1.to_owned()))
        .collect();
    spent_current_year_by_payment_method.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().reverse());

    return Stats {
        average_spending_per_day,
        spent_last_month,
        spent_last_month_by_category,
        spent_last_year,
        spent_last_year_by_category,
        spent_current_year,
        spent_current_year_by_category,
        spent_current_year_by_payment_method,
        spent_current_year_by_month,
        spent_current_year_per_day,
    };
}

fn print_stats(
    stats: &Stats,
    last_30_days: bool,
    last_month: bool,
    last_year: bool,
    current_year: bool,
) {
    println!("---");
    if last_30_days {
        let to_skip = (stats.average_spending_per_day.len() as isize - 30).max(0);
        for (date, spent) in stats.average_spending_per_day.iter().skip(to_skip as usize) {
            println!("{}: {:.2}", date.format("%d/%m/%Y"), spent);
        }
        println!("---");
    }

    if last_month {
        println!(
            "Spent last month: {:.2} ({:.2} per day)",
            stats.spent_last_month,
            stats.spent_last_month / 30.0
        );
        for (category, spent) in stats.spent_last_month_by_category.iter() {
            println!("  {:15}: {:.2}", category.to_string(), spent);
        }
        println!("---");
    }

    if last_year {
        println!(
            "Spent last year: {:.2} ({:.2} per day)",
            stats.spent_last_year,
            stats.spent_last_year / 365.0
        );
        for (category, spent) in stats.spent_last_year_by_category.iter() {
            println!("  {:15}: {:.2}", category.to_string(), spent);
        }
        println!("---");
    }

    if current_year {
        println!("Spent current year");
        println!(
            "  {:.2} ({:.2} per day)",
            stats.spent_current_year, stats.spent_current_year_per_day
        );
        println!("  ---");
        let spent_by_cat = stats
            .spent_current_year_by_category
            .iter()
            .map(|(a, b, c)| (a.to_string(), b, c));
        let max_len = spent_by_cat.clone().map(|x| x.0.len()).max().unwrap();
        for (category, spent, percentage) in spent_by_cat {
            println!(
                "  {:<3$}: {:7.2} ({:5.2}%)",
                category,
                spent,
                percentage * 100.0,
                max_len
            );
        }
        println!("  ---");
        for (pm, spent) in stats.spent_current_year_by_payment_method.iter() {
            println!("  {:9}: {:.2}", pm, spent);
        }
        println!("  ---");
        for (month, spent, per_day) in stats.spent_current_year_by_month.iter() {
            let month = NaiveDate::from_ymd_opt(1, *month, 1).unwrap().format("%B");
            println!("  {:9}: {:6.2} ({:5.2})", month, spent, per_day);
        }
        println!("---");
    }
}

fn plot_monthly_usage(filepath: &PathBuf, entries: &Vec<Entry>) {
    let today = Utc::now().date_naive();
    let mut monthly_spending = HashMap::<(i32, u32), f64>::new();

    let mut max_value: f64 = 0.0;
    let magic_factor = 1.1;
    let first = entries.first().unwrap();
    let last = entries.last().unwrap();
    let start_year = first.date.year_ce().1 as i32 * if first.date.year_ce().0 { 1 } else { -1 };
    let start_month = first.date.month0();
    let end_year = last.date.year_ce().1 as i32 * if last.date.year_ce().0 { 1 } else { -1 };
    let end_month = last.date.month0();

    let num_months = end_year * 12 - start_year * 12 + end_month as i32 - start_month as i32;

    for e in entries.iter() {
        let year = e.date.year_ce().1 as i32 * if e.date.year_ce().0 { 1 } else { -1 };
        let month = e.date.month0() + 1;
        let cents = e.value.1 as f64;
        let value = e.value.0 as f64 + cents / 10.0_f64.powf((cents + 1.0).log10().ceil());
        let value = value
            / (NaiveDate::from_ymd_opt(year + if month == 12 { 1 } else { 0 }, (month % 12) + 1, 1)
                .unwrap()
                .min(today)
                - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
            .num_days() as f64;

        let prev = monthly_spending.get(&(year, month)).unwrap_or(&0.0);
        max_value = max_value.max(prev + value);
        monthly_spending.insert((year, month), prev + value);
    }

    let mut monthly_values = monthly_spending.into_iter().collect::<Vec<_>>();
    monthly_values
        .sort_by(|a, b| (a.0 .0 * 13 + a.0 .1 as i32).cmp(&(b.0 .0 * 13 + b.0 .1 as i32)));
    let monthly_values = monthly_values.iter().map(|x| x.1).collect::<Vec<_>>();

    let month_labels = (0..=num_months).map(|x| {
        let n = x + start_month as i32;
        let year = start_year + n / 12;
        let month = n % 12 + 1;
        format!("{:02}/{}", month, year)
    });

    let month_value_labels = (0..=num_months).map(|x| format!("{:.2}", monthly_values[x as usize]));

    let root = BitMapBackend::new(filepath, (960 * 2, 720 * 2)).into_drawing_area();
    root.fill(&WHITE).unwrap();

    // Create a chart builder
    let mut chart = ChartBuilder::on(&root)
        .caption("Monthly Spending", ("serif", 64).into_font())
        .x_label_area_size(100)
        .y_label_area_size(100)
        .right_y_label_area_size(100)
        .margin(50)
        .build_cartesian_2d(
            (0.0..((num_months + 1) as f32)).step(1.0),
            0.0..max_value * magic_factor,
        )
        .unwrap();

    // Configure the axes
    chart
        .configure_mesh()
        .x_desc("Months")
        .y_desc("Daily average")
        .axis_desc_style(("serif", 32).into_font())
        .x_label_style(("serif", 24).into_font())
        .y_label_style(("serif", 24).into_font())
        .x_labels((num_months + 1) as usize)
        .y_labels(10)
        .x_label_formatter(&|_| String::default())
        .draw()
        .unwrap();

    chart
        .draw_series(monthly_values.iter().enumerate().map(|(month, &value)| {
            Rectangle::new(
                [(month as f32, 0.0), ((month + 1) as f32, value as f64)],
                RED.mix((value / max_value).sqrt()).filled(),
            )
        }))
        .unwrap();

    let font = ("serif", 28.0).into_font();
    let pixels_per_unit_x =
        chart.plotting_area().get_x_axis_pixel_range().len() as f32 / num_months as f32;
    let pixels_per_unit_y = chart.plotting_area().get_y_axis_pixel_range().len() as f64 / (max_value * magic_factor);

    for (i, label) in month_labels.into_iter().enumerate() {
        let offset_x = (font.box_size(&label).unwrap().0 as f32) / pixels_per_unit_x;
        // let offset_y = (font.box_size(&label).unwrap().1 as f64) / pixels_per_unit_y;
        chart
            .draw_series(std::iter::once(Text::new(
                label,
                (i as f32 + 0.5 - offset_x * 0.5, -20.0), // Positioning the label
                font.clone(),
            )))
            .unwrap();
    }

    for (i, label) in month_value_labels.into_iter().enumerate() {
        let offset_x = (font.box_size(&label).unwrap().0 as f32) / pixels_per_unit_x;
        // let offset_y = (font.box_size(&label).unwrap().1 as f64) / pixels_per_unit_y;
        chart
            .draw_series(std::iter::once(Text::new(
                label,
                (i as f32 + 0.5 - offset_x * 0.5, monthly_values[i] / 2.0), // Positioning the label
                font.clone(),
            )))
            .unwrap();
    }

    let mut pts = moving_average(monthly_values, 12)
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f32 + 0.5, *v))
        .collect::<Vec<_>>();

    pts.insert(0, (0.0, pts.first().unwrap().1));
    pts.push(((num_months + 1) as f32, pts.last().unwrap().1));

    chart
        .draw_series(LineSeries::new(pts.clone().into_iter(), AMBER.stroke_width(10)))
        .unwrap();

    {
        let value =  pts.last().unwrap().1;
        let label = format!("Average: {:.2}", value);
        let offset_x = (font.box_size(&label).unwrap().0 as f32) / pixels_per_unit_x;
        let offset_y = (font.box_size(&label).unwrap().1 as f64) / pixels_per_unit_y;
        chart
            .draw_series(std::iter::once(Text::new(
                label,
                ((num_months as f32 + 1.0) - offset_x - 20.0/pixels_per_unit_x, value + offset_y*1.5), // Positioning the label
                font.clone().style(FontStyle::Bold),
            )))
            .unwrap();
    }

    root.present().unwrap();
}

fn main() {
    let path = get_path();

    if path.is_none() {
        eprintln!("[ERROR] No file provided.");
        print_usage();
        return;
    }

    assert!(path.is_some(), "Rust has a problem here.");
    let path = path.unwrap();
    let entries = parse_file(&path);

    if entries.is_empty() {
        println!("[INFO] Provided file has no entries. Exiting...");
        return;
    }

    let stats = gather_stats(&entries);
    print_stats(&stats, false, false, false, true);

    let mut out_file_path = path.clone();
    out_file_path.set_extension("png");
    plot_monthly_usage(&out_file_path, &entries);
    println!("Monthly usage chart saved in `{}`.", out_file_path.display());
}
