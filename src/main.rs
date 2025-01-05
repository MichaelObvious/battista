use std::{
    cmp::Ordering,
    collections::HashMap,
    env,
    fmt::{self, Debug},
    fs,
    hash::Hash,
    io::Write,
    path::PathBuf,
    process::exit,
    vec,
};

use chrono::{Datelike, Local, NaiveDate, TimeDelta};
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
    Fine,
    Gift,
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
struct Transaction {
    value: i64, // units and cents
    date: NaiveDate,
    category: Category,
    end_date: NaiveDate,
    payment_method: String,
    note: String,
}

#[derive(Debug, Default)]
struct Stats {
    per_day: f64,
    total: i64,
    by_category: Vec<(Category, i64)>,
    by_payment_method: Vec<(String, i64)>,
    by_note: Vec<(String, i64)>,
    average_transaction: f64,
    transaction_count: u64,
}

#[derive(Debug, Default)]
struct TempStats {
    per_day: f64,
    total: i64,
    by_category: HashMap<Category, i64>,
    by_payment_method: HashMap<String, i64>,
    by_note: HashMap<String, i64>,
    average_transaction: f64,
    transaction_count: u64,
}

impl TempStats {
    pub fn update(&mut self, e: &Transaction) {
        let value = e.value;
        self.total += value;
        if !self.by_category.contains_key(&e.category) {
            self.by_category.insert(e.category.clone(), 0);
        }
        *(self.by_category.get_mut(&e.category).unwrap()) += value;

        if !self.by_payment_method.contains_key(&e.payment_method) {
            self.by_payment_method.insert(e.payment_method.clone(), 0);
        }
        *(self.by_payment_method.get_mut(&e.payment_method).unwrap()) += value;

        if !self.by_note.contains_key(&e.note) {
            self.by_note.insert(e.note.clone(), 0);
        }
        *(self.by_note.get_mut(&e.note).unwrap()) += value;

        self.transaction_count += 1;
    }

    pub fn calc_averages(&mut self, days: i64) {
        let days = days as f64;
        self.per_day = self.get_total() / days;
        self.average_transaction = self.get_total() / self.transaction_count as f64;
    }

    pub fn into_stats(self) -> Stats {
        let mut by_category = self.by_category.into_iter().collect::<Vec<_>>();
        by_category.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_payment_method = self.by_payment_method.into_iter().collect::<Vec<_>>();
        by_payment_method.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        let mut by_note = self.by_note.into_iter().collect::<Vec<_>>();
        by_note.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap().reverse());
        Stats {
            per_day: self.per_day,
            total: self.total,
            by_category,
            by_payment_method,
            by_note,
            average_transaction: self.average_transaction,
            transaction_count: self.transaction_count,
        }
    }

    fn get_total(&self) -> f64 {
        self.total as f64 / 100.0
    }
}

impl Stats {
    fn get_total(&self) -> f64 {
        self.total as f64 / 100.0
    }
}

#[derive(Debug, Default)]
struct StatsCollection {
    yearly: Vec<(i32, Stats)>,         // year
    monthly: Vec<((i32, u32), Stats)>, // year, month
    last_365_days: Stats,
    last_30_days: Stats,
}

#[derive(Debug, Default)]
struct TempStatsCollection {
    yearly: HashMap<i32, TempStats>,         // year
    monthly: HashMap<(i32, u32), TempStats>, // year, month
    last_365_days: TempStats,
    last_30_days: TempStats,
}

impl TempStatsCollection {
    pub fn into_stats_collection(self) -> StatsCollection {
        let mut yearly = self
            .yearly
            .into_iter()
            .map(|(a, b)| (a, b.into_stats()))
            .collect::<Vec<_>>();
        yearly.sort_by(|x, y| x.0.cmp(&y.0));
        let mut monthly = self
            .monthly
            .into_iter()
            .map(|(a, b)| (a, b.into_stats()))
            .collect::<Vec<_>>();
        monthly.sort_by(|x, y| (x.0 .0 * 12 + x.0 .1 as i32).cmp(&(y.0 .0 * 12 + y.0 .1 as i32)));
        StatsCollection {
            yearly: yearly,
            monthly: monthly,
            last_30_days: self.last_30_days.into_stats(),
            last_365_days: self.last_365_days.into_stats(),
        }
    }
}

// struct DateRange(NaiveDate, NaiveDate);

// impl Iterator for DateRange {
//     type Item = NaiveDate;

//     fn next(&mut self) -> Option<Self::Item> {
//         if self.0 <= self.1 {
//             let next = self.0 + Duration::days(1);
//             Some(mem::replace(&mut self.0, next))
//         } else {
//             None
//         }
//     }
// }

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

fn weighted_moving_average(xs: Vec<(f64, f64)>, window: isize) -> Vec<f64> {
    let mut average = Vec::new();

    for i in 0..xs.len() {
        let mut a = 0.0;
        let mut d = 0.0;
        let start = (i as isize - window + 1).max(0) as usize;
        for j in start..=i {
            a += xs[j].0 * xs[j].1;
            d += xs[j].1;
        }
        a /= d;
        average.push(a);
    }
    assert!(average.len() == xs.len());
    return average;
}

fn days_in_month(d: NaiveDate) -> i64 {
    let year = year_as_i32(d.year_ce());
    let month = d.month0() + 1;
    (NaiveDate::from_ymd_opt(year + if month == 12 { 1 } else { 0 }, (month % 12) + 1, 1).unwrap()
        - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days()
}

fn days_in_year(d: NaiveDate) -> i64 {
    let year = year_as_i32(d.year_ce());
    let month = d.month0() + 1;
    (NaiveDate::from_ymd_opt(year + 1, month, 1).unwrap()
        - NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days()
}


fn year_as_i32(year_ce: (bool, u32)) -> i32 {
    if year_ce.0 {
        year_ce.1 as i32
    } else {
        -1 * year_ce.1 as i32
    }
}

fn escape_string_for_tex(str: &String) -> String {
    str.replace('&', "\\&").replace('$', "\\$")
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

fn parse_file(filepath: &PathBuf) -> Vec<Transaction> {
    let content = fs::read_to_string(&filepath).unwrap_or_default();
    let lines = content.lines().skip(1);

    let mut transactions = vec![];

    for (line_idx, line) in lines.enumerate() {
        let fields = line.split(';');
        let mut transaction = Transaction::default();
        for (field_idx, field) in fields.enumerate() {
            match field_idx {
                0 => {
                    let negative = field.trim().starts_with('-');
                    let mut parts = field.split('.');
                    let units = parts.next().unwrap().trim().parse::<i32>().unwrap();
                    let cents = parts
                        .next()
                        .unwrap_or("0")
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(0);

                    if cents >= 100 {
                        eprintln!(
                            "[ERROR] Could not parse amount `{}` in {}:{} (cents seem to have too many digits).",
                            field.trim(),
                            filepath.display(),
                            line_idx + 2
                        );
                        exit(1);
                    }
                    let cents = if units < 0 || negative {
                        -(cents as i64)
                    } else {
                        cents as i64
                    } * if cents < 10 { 10 } else { 1 };
                    transaction.value = units as i64 * 100 + cents;
                }
                1 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field.trim(), "%d/%m/%Y") {
                        transaction.date = date;
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
                    transaction.category = Category::from(field.trim());
                }
                3 => {
                    if let Ok(date) = NaiveDate::parse_from_str(field.trim(), "%d/%m/%Y") {
                        transaction.end_date = date;
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
                    transaction.payment_method = String::from(field.trim());
                }
                5 => {
                    transaction.note = String::from(field.trim());
                }
                _ => {}
            }
        }

        if Ordering::is_gt(transaction.date.cmp(&transaction.end_date)) {
            eprintln!(
                "[ERROR] Date is later than end date in {}:{}",
                filepath.display(),
                line_idx + 2
            );
            exit(1);
        }

        transactions.push(transaction);
    }

    transactions.sort_by(|a, b| a.date.cmp(&b.date));

    return transactions;
}

fn get_stats(transactions: &Vec<Transaction>) -> StatsCollection {
    let mut tsc = TempStatsCollection::default();
    let today = Local::now().date_naive();

    let mut start = today;
    for transaction in transactions.iter() {
        let year = year_as_i32(transaction.date.year_ce());
        let month = transaction.date.month0() + 1;
        start = start.min(transaction.date);

        // Yearly
        if !tsc.yearly.contains_key(&year) {
            tsc.yearly.insert(year, TempStats::default());
        }
        tsc.yearly.get_mut(&year).unwrap().update(transaction);

        // Monthly
        let month_idx = (year, month);
        if !tsc.monthly.contains_key(&month_idx) {
            tsc.monthly.insert(month_idx, TempStats::default());
        }
        tsc.monthly.get_mut(&month_idx).unwrap().update(transaction);

        if (today - transaction.date).num_days() <= 30 {
            tsc.last_30_days.update(transaction);
        }

        if (today - transaction.date).num_days() <= 365 {
            tsc.last_365_days.update(transaction);
        }
    }

    for (k, v) in tsc.yearly.iter_mut() {
        let year_start = NaiveDate::from_ymd_opt(*k, 1, 1).unwrap();
        let period_start = year_start.max(start);
        let period_end = (NaiveDate::from_ymd_opt(*k + 1, 1, 1).unwrap() - TimeDelta::days(1))
            .min(today + TimeDelta::days(1));
        let days = days_in_year(year_start);
        let days2 = (period_end - period_start).num_days();
        // println!("{} {} {} {} {}", year_start, period_start, period_end, days, days2);
        v.calc_averages(days.min(days2));
    }

    for (k, v) in tsc.monthly.iter_mut() {
        let month_start = NaiveDate::from_ymd_opt(k.0, k.1, 1).unwrap();

        let month_end =
            NaiveDate::from_ymd_opt(k.0 + if k.1 == 12 { 1 } else { 0 }, (k.1 % 12) + 1, 1)
                .unwrap()
                - TimeDelta::days(1);
        let period_start = month_start.max(start);
        let period_end = (month_end + TimeDelta::days(1)).min(today + TimeDelta::days(1));
        let days = days_in_month(month_start);
        let days2 = (period_end - period_start).num_days();
        // println!("{} {} {} {} {} {}", month_start, month_end, period_start, period_end, days, days2);
        v.calc_averages(days.min(days2));
    }

    tsc.last_30_days.calc_averages(30);
    tsc.last_365_days.calc_averages(365);

    return tsc.into_stats_collection();
}

fn print_stats(stats: &StatsCollection) {
    let today = Local::now().date_naive();

    println!("SPENDING REPORT");
    println!("===============");

    let mut this_year = None;
    for (year, yearly) in stats.yearly.iter() {
        if *year == year_as_i32(today.year_ce()) {
            this_year = Some(yearly)
        };
        println!(
            "  - {}: {:.2} ({:.2} per day)",
            year,
            yearly.get_total(),
            yearly.per_day
        );
    }

    if let Some(this_year) = this_year {
        println!("    - Categories:");
        let max_len = this_year
            .by_category
            .iter()
            .map(|x| x.0.to_string().len())
            .max()
            .unwrap_or_default();
        for (c, v) in this_year.by_category.iter() {
            let percentage = (*v as f64 / this_year.total as f64) * 100.0;
            println!(
                "       - {:<3$}: {:7.2} ({:5.2}%)",
                c.to_string(),
                *v as f64 / 100.0,
                percentage,
                max_len
            );
        }

        println!("    - Payment methods:");
        let max_len = this_year
            .by_payment_method
            .iter()
            .map(|x| x.0.len())
            .max()
            .unwrap_or_default();
        for (pm, v) in this_year.by_payment_method.iter() {
            let percentage = (*v as f64 / this_year.total as f64) * 100.0;
            println!(
                "       - {:<3$}: {:7.2} ({:5.2}%)",
                pm,
                *v as f64 / 100.0,
                percentage,
                max_len
            );
        }
    }

    let mut this_month = None;
    println!("    - Months:");
    for ((y, m), monthly) in stats.monthly.iter() {
        if *y != year_as_i32(today.year_ce()) {
            continue;
        }
        if *m == today.month0() + 1 {
            this_month = Some(monthly)
        }
        let month_name = NaiveDate::from_ymd_opt(*y, *m, 1).unwrap().format("%B");
        println!(
            "      - {:9}: {:7.2} ({:5.2} per day)",
            month_name,
            monthly.get_total(),
            monthly.per_day
        );
    }

    if let Some(this_month) = this_month {
        println!("        - Categories:");
        let max_len = this_month
            .by_category
            .iter()
            .map(|x| x.0.to_string().len())
            .max()
            .unwrap_or_default();
        for (c, v) in this_month.by_category.iter() {
            let percentage = (*v as f64 / this_month.total as f64) * 100.0;
            println!(
                "           - {:<3$}: {:7.2} ({:5.2}%)",
                c.to_string(),
                *v as f64 / 100.0,
                percentage,
                max_len
            );
        }
    }
    println!();
    println!(
        "Spent last 365 days: {:.2} ({:.2} per day)",
        stats.last_365_days.get_total(),
        stats.last_365_days.per_day
    );
    println!(
        "Spent last 30 days: {:.2} ({:.2} per day)",
        stats.last_30_days.get_total(),
        stats.last_30_days.per_day
    );
    println!();
    println!("===============");
}

fn write_tex_stats(file_path: &PathBuf, stats: &StatsCollection, original_path: &PathBuf) {
    let today_date_formatted = Local::now().date_naive().format("%B %d, %Y");

    let mut buf = Vec::new();
    writeln!(buf, "\\documentclass[10pt, a4paper]{{article}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "\\usepackage[english]{{babel}}").unwrap();
    writeln!(buf, "\\usepackage{{csquotes}}").unwrap();
    writeln!(buf, "\\usepackage[portrait]{{geometry}}").unwrap();
    writeln!(buf, "\\usepackage{{hyperref}}").unwrap();
    writeln!(buf, "\\usepackage{{longtable}}").unwrap();
    writeln!(buf, "\\usepackage{{microtype}}").unwrap();
    writeln!(buf, "\\usepackage{{pgfplots}}").unwrap();
    writeln!(buf).unwrap();

    writeln!(buf, "\\hypersetup{{").unwrap();
    writeln!(buf, "    colorlinks=true,").unwrap();
    writeln!(buf, "    linkcolor=black,").unwrap();
    writeln!(buf, "    urlcolor=black,").unwrap();
    writeln!(buf, "    bookmarks=true,").unwrap();
    writeln!(buf, "    pdftitle={{Spending report from {} ({})}},",  original_path.display(), today_date_formatted).unwrap();
    writeln!(buf, "}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(
        buf,
        "\\title{{\\textbf{{Spending report from}} \\texttt{{{}}}}}",
        original_path.display()
    )
    .unwrap();
    writeln!(
        buf,
        "\\author{{\\href{{{}}}{{{}}} {}}}",
        "https://www.github.com/MichaelObvious/battista",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
    .unwrap();
    writeln!(
        buf,
        "\\date{{{}}}",
        today_date_formatted
    )
    .unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "\\makeindex").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "\\begin{{document}}").unwrap();
    writeln!(buf, "  \\maketitle").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "\\tableofcontents").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\vspace{{5ex}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\section{{Overview}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{tikzpicture}}").unwrap();
    writeln!(buf, "    \\small").unwrap();
    writeln!(buf, "    \\begin{{axis}}[").unwrap();
    writeln!(
        buf,
        "      symbolic y coords={{{}}},",
        stats
            .monthly
            .iter()
            .map(|((y, m), _)| format!("{:02}/{}", m, y % 100))
            .rev()
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();
    writeln!(buf, "      xbar,").unwrap();
    writeln!(buf, "      ytick=data,").unwrap();
    writeln!(buf, "      width=\\textwidth,").unwrap();
    writeln!(buf, "      nodes near coords,").unwrap();
    writeln!(
        buf,
        "      every node near coord/.append style={{anchor=west,font=\\tiny}},"
    )
    .unwrap();
    writeln!(buf, "      xlabel={{Daily Average}},").unwrap();
    writeln!(buf, "      enlarge x limits={{value=0.2,upper}},").unwrap();
    writeln!(buf, "      xmin=0").unwrap();
    writeln!(buf, "    ]").unwrap();
    writeln!(buf, "\\addplot[xbar, fill=black!20] coordinates {{").unwrap();
    // let start_year = stats.monthly.first().unwrap().0 .0;
    // let start_month = stats.monthly.first().unwrap().0.1;
    for ((y, m), monthly) in stats.monthly.iter() {
        // let month_name = NaiveDate::from_ymd_opt(*y, *m, 1).unwrap().format("%B");
        writeln!(buf, "      ({},{:02}/{})", monthly.per_day, m, y % 100).unwrap();
    }
    writeln!(buf, "}};").unwrap();

    writeln!(buf, "\\addplot[smooth, black!67,").unwrap();
    // writeln!(
    //     buf,
    //     "      every node near coord/.append style={{anchor=east,font=\\tiny}},"
    // )
    // .unwrap();
    writeln!(buf, "] coordinates {{").unwrap();
    let values = stats.monthly.iter().map(|x| x.1.per_day).collect();
    for (value, (y, m)) in moving_average(values, 12)
        .into_iter()
        .zip(stats.monthly.iter().map(|x| x.0))
    {
        // let idx = (y - start_year) * 12 + (m - start_month) as i32;
        writeln!(buf, "      ({},{:02}/{})", value, m, y % 100).unwrap();
    }
    writeln!(buf, "}};").unwrap();
    // writeln!(buf, "    \\centering").unwrap();
    // writeln!(buf, "    \\includegraphics[width=\\textwidth]{{{}}}", image_path.display()).unwrap();
    writeln!(buf, "  \\end{{axis}}").unwrap();
    writeln!(buf, "  \\end{{tikzpicture}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{Last 30 days}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{itemize}}").unwrap();
    writeln!(
        buf,
        "    \\item {:.2} spent ({:.2} in average per day);",
        stats.last_30_days.get_total(),
        stats.last_30_days.per_day
    )
    .unwrap();
    writeln!(
        buf,
        "    \\item {} transactions ({:.2} in average per transaction).",
        stats.last_30_days.transaction_count, stats.last_30_days.average_transaction
    )
    .unwrap();
    writeln!(buf, "  \\end{{itemize}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{Last 365 days}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{itemize}}").unwrap();
    writeln!(
        buf,
        "    \\item {:.2} spent ({:.2} in average per day);",
        stats.last_365_days.get_total(),
        stats.last_365_days.per_day
    )
    .unwrap();
    writeln!(
        buf,
        "    \\item {} transactions ({:.2} in average per transaction).",
        stats.last_365_days.transaction_count, stats.last_365_days.average_transaction
    )
    .unwrap();
    writeln!(buf, "  \\end{{itemize}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\section{{Yearly spending}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    writeln!(buf, "      \\hline").unwrap();
    writeln!(
        buf,
        "      \\textbf{{Year}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Daily Average}}}}\\\\"
    )
    .unwrap();
    writeln!(buf, "      \\hline").unwrap();
    writeln!(buf, "      \\hline").unwrap();
    for (year, yearly) in stats.yearly.iter() {
        writeln!(
            buf,
            "      {} & {:.2} & {:.2}\\\\",
            year,
            yearly.get_total(),
            yearly.per_day
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{By Category}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    for (year, yearly) in stats.yearly.iter() {
        writeln!(buf, "      \\hline").unwrap();
        writeln!(
            buf,
            "      \\multicolumn{{3}}{{c}}{{\\textbf{{{}}}}}\\\\",
            year
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
        writeln!(buf, "      \\textbf{{Category}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Percentage}}}}\\\\").unwrap();
        writeln!(buf, "      \\hline").unwrap();
        for (cat, value) in yearly.by_category.iter() {
            let percentage = (*value as f64 / yearly.total as f64) * 100.0;
            if percentage > 100.0 - 1e-3 {
                writeln!(
                    buf,
                    "      {} & {:.2} & {}\\% \\\\",
                    cat,
                    *value as f64 / 100.0,
                    100
                )
                .unwrap();
            } else {
                writeln!(
                    buf,
                    "      {} & {:.2} & {:.2}\\% \\\\",
                    cat,
                    *value as f64 / 100.0,
                    percentage
                )
                .unwrap();
            }
            writeln!(buf, "      \\hline").unwrap();
        }
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{By Payment method}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    for (year, yearly) in stats.yearly.iter() {
        writeln!(buf, "      \\hline").unwrap();
        writeln!(
            buf,
            "      \\multicolumn{{3}}{{c}}{{\\textbf{{{}}}}}\\\\",
            year
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
        writeln!(buf, "      \\textbf{{Payment method}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Percentage}}}}\\\\").unwrap();
        writeln!(buf, "      \\hline").unwrap();
        for (pm, value) in yearly.by_payment_method.iter() {
            let percentage = (*value as f64 / yearly.total as f64) * 100.0;
            if percentage > 100.0 - 1e-3 {
                writeln!(
                    buf,
                    "      {} & {:.2} & {}\\% \\\\",
                    pm,
                    *value as f64 / 100.0,
                    100
                )
                .unwrap();
            } else {
                writeln!(
                    buf,
                    "      {} & {:.2} & {:.2}\\% \\\\",
                    pm,
                    *value as f64 / 100.0,
                    percentage
                )
                .unwrap();
            }
            writeln!(buf, "      \\hline").unwrap();
        }
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{By Note}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    for (year, yearly) in stats.yearly.iter() {
        writeln!(buf, "      \\hline").unwrap();
        writeln!(
            buf,
            "      \\multicolumn{{3}}{{c}}{{\\textbf{{{}}}}}\\\\",
            year
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
        writeln!(buf, "      \\textbf{{Note}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Percentage}}}}\\\\").unwrap();
        writeln!(buf, "      \\hline").unwrap();
        for (note, value) in yearly.by_note.iter() {
            let note = escape_string_for_tex(note);
            let percentage = (*value as f64 / yearly.total as f64) * 100.0;
            if percentage > 100.0 - 1e-3 {
                writeln!(
                    buf,
                    "      \\textquote{{{}}} & {:.2} & {}\\% \\\\",
                    note,
                    *value as f64 / 100.0,
                    100
                )
                .unwrap();
            } else {
                writeln!(
                    buf,
                    "      \\textquote{{{}}} & {:.2} & {:.2}\\% \\\\",
                    note,
                    *value as f64 / 100.0,
                    percentage
                )
                .unwrap();
            }
            writeln!(buf, "      \\hline").unwrap();
        }
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\section{{Monthly spending}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    writeln!(buf, "      \\hline").unwrap();
    writeln!(
        buf,
        "      \\textbf{{Month}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Daily average}}}}\\\\"
    )
    .unwrap();
    writeln!(buf, "      \\hline").unwrap();
    writeln!(buf, "      \\hline").unwrap();
    for ((y, m), monthly) in stats.monthly.iter() {
        let month_name = NaiveDate::from_ymd_opt(*y, *m, 1).unwrap().format("%B");
        writeln!(
            buf,
            "      {} {} & {:.2} & {:.2}\\\\",
            month_name,
            y,
            monthly.get_total(),
            monthly.per_day
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{By Category}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    for ((y, m), monthly) in stats.monthly.iter() {
        let month_name = NaiveDate::from_ymd_opt(*y, *m, 1).unwrap().format("%B");
        writeln!(buf, "      \\hline").unwrap();
        writeln!(
            buf,
            "      \\multicolumn{{3}}{{c}}{{\\textbf{{{} {}}}}}\\\\",
            month_name, y
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
        writeln!(buf, "      \\textbf{{Category}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Percentage}}}}\\\\").unwrap();
        writeln!(buf, "      \\hline").unwrap();
        for (cat, value) in monthly.by_category.iter() {
            let percentage = (*value as f64 / monthly.total as f64) * 100.0;
            if percentage > 100.0 - 1e-3 {
                writeln!(
                    buf,
                    "      {} & {:.2} & {}\\% \\\\",
                    cat,
                    *value as f64 / 100.0,
                    100
                )
                .unwrap();
            } else {
                writeln!(
                    buf,
                    "      {} & {:.2} & {:.2}\\% \\\\",
                    cat,
                    *value as f64 / 100.0,
                    percentage
                )
                .unwrap();
            }
            writeln!(buf, "      \\hline").unwrap();
        }
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{By Payment method}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    for ((y, m), monthly) in stats.monthly.iter() {
        let month_name = NaiveDate::from_ymd_opt(*y, *m, 1).unwrap().format("%B");
        writeln!(buf, "      \\hline").unwrap();
        writeln!(
            buf,
            "      \\multicolumn{{3}}{{c}}{{\\textbf{{{} {}}}}}\\\\",
            month_name, y
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
        writeln!(buf, "      \\textbf{{Payment method}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Percentage}}}}\\\\").unwrap();
        writeln!(buf, "      \\hline").unwrap();
        for (pm, value) in monthly.by_payment_method.iter() {
            let percentage = (*value as f64 / monthly.total as f64) * 100.0;
            if percentage > 100.0 - 1e-3 {
                writeln!(
                    buf,
                    "      {} & {:.2} & {}\\% \\\\",
                    pm,
                    *value as f64 / 100.0,
                    100
                )
                .unwrap();
            } else {
                writeln!(
                    buf,
                    "      {} & {:.2} & {:.2}\\% \\\\",
                    pm,
                    *value as f64 / 100.0,
                    percentage
                )
                .unwrap();
            }
            writeln!(buf, "      \\hline").unwrap();
        }
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\subsection{{By Note}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "  \\begin{{center}}").unwrap();
    writeln!(buf, "    \\begin{{longtable}}{{l r r}}").unwrap();
    for ((y, m), monthly) in stats.monthly.iter() {
        let month_name = NaiveDate::from_ymd_opt(*y, *m, 1).unwrap().format("%B");
        writeln!(buf, "      \\hline").unwrap();
        writeln!(
            buf,
            "      \\multicolumn{{3}}{{c}}{{\\textbf{{{} {}}}}}\\\\",
            month_name, y
        )
        .unwrap();
        writeln!(buf, "      \\hline").unwrap();
        writeln!(buf, "      \\textbf{{Note}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Spent}}}} & \\multicolumn{{1}}{{l}}{{\\textbf{{Percentage}}}}\\\\").unwrap();
        writeln!(buf, "      \\hline").unwrap();
        for (note, value) in monthly.by_note.iter() {
            let note = escape_string_for_tex(note);
            let percentage = (*value as f64 / monthly.total as f64) * 100.0;
            if percentage > 100.0 - 1e-3 {
                writeln!(
                    buf,
                    "       \\textquote{{{}}} & {:.2} & {}\\% \\\\",
                    note,
                    *value as f64 / 100.0,
                    100
                )
                .unwrap();
            } else {
                writeln!(
                    buf,
                    "       \\textquote{{{}}} & {:.2} & {:.2}\\% \\\\",
                    note,
                    *value as f64 / 100.0,
                    percentage
                )
                .unwrap();
            }
            writeln!(buf, "      \\hline").unwrap();
        }
    }
    writeln!(buf, "    \\end{{longtable}}").unwrap();
    writeln!(buf, "  \\end{{center}}").unwrap();
    writeln!(buf).unwrap();
    writeln!(buf, "\\end{{document}}").unwrap();
    let mut f = std::fs::File::create(file_path).unwrap();
    f.write(buf.as_slice()).unwrap();
}

fn plot_monthly_usage(
    filepath: &PathBuf,
    transactions: &Vec<Transaction>,
    stats: &StatsCollection,
) {
    let max_value: f64 = stats
        .monthly
        .iter()
        .map(|(_, b)| b.per_day)
        .max_by(|a, b| a.partial_cmp(&b).unwrap())
        .unwrap();
    let magic_factor = 1.1;
    let first = transactions.first().unwrap();
    let last = transactions.last().unwrap();
    let start_year = first.date.year_ce().1 as i32 * if first.date.year_ce().0 { 1 } else { -1 };
    let start_month = first.date.month0();
    let end_year = last.date.year_ce().1 as i32 * if last.date.year_ce().0 { 1 } else { -1 };
    let end_month = last.date.month0();

    let num_months = end_year * 12 - start_year * 12 + end_month as i32 - start_month as i32;

    let monthly_values = stats
        .monthly
        .iter()
        .map(|x| x.1.per_day)
        .collect::<Vec<_>>();

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
        .draw_series(monthly_values.iter().enumerate().map(|(month, &v)| {
            Rectangle::new(
                [(month as f32, 0.0), ((month + 1) as f32, v as f64)],
                RED.mix((v / max_value).sqrt()).filled(),
            )
        }))
        .unwrap();

    let font = ("serif", 28.0).into_font();
    let pixels_per_unit_x =
        chart.plotting_area().get_x_axis_pixel_range().len() as f32 / num_months as f32;
    let pixels_per_unit_y =
        chart.plotting_area().get_y_axis_pixel_range().len() as f64 / (max_value * magic_factor);

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
        let offset_y = (font.box_size(&label).unwrap().1 as f64) / pixels_per_unit_y;
        chart
            .draw_series(std::iter::once(Text::new(
                label,
                (
                    i as f32 + 0.5 - offset_x * 0.5,
                    (monthly_values[i] + offset_y) / 2.0,
                ), // Positioning the label
                font.clone(),
            )))
            .unwrap();
    }

    let mut pts = if true {
        let values = stats
            .monthly
            .iter()
            .map(|(a, b)| {
                (
                    b.per_day,
                    days_in_month(NaiveDate::from_ymd_opt(a.0, a.1, 1).unwrap()) as f64,
                )
            })
            .collect();
        weighted_moving_average(values, 12)
    } else {
        let values = stats.monthly.iter().map(|x| x.1.per_day).collect();
        moving_average(values, 12)
    }
    .iter()
    .enumerate()
    .map(|(i, v)| (i as f32 + 0.5, *v))
    .collect::<Vec<_>>();

    pts.insert(0, (0.0, pts.first().unwrap().1));
    pts.push(((num_months + 1) as f32, pts.last().unwrap().1));

    chart
        .draw_series(LineSeries::new(
            pts.clone().into_iter(),
            AMBER.stroke_width(10),
        ))
        .unwrap();

    {
        let value = pts.last().unwrap().1;
        let label = format!("Average: {:.2}", value);
        let offset_x = (font.box_size(&label).unwrap().0 as f32) / pixels_per_unit_x;
        let offset_y = (font.box_size(&label).unwrap().1 as f64) / pixels_per_unit_y;
        chart
            .draw_series(std::iter::once(Text::new(
                label,
                (
                    (num_months as f32 + 1.0) - offset_x - 20.0 / pixels_per_unit_x,
                    value + offset_y * 1.5,
                ), // Positioning the label
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
    let transactions = parse_file(&path);

    if transactions.is_empty() {
        println!("[INFO] Provided file has no transactions. Exiting...");
        return;
    }

    let stats = get_stats(&transactions);
    print_stats(&stats);

    if false {
        let mut out_graph_path = path.clone();
        out_graph_path.set_extension("png");
        plot_monthly_usage(&out_graph_path, &transactions, &stats);
        println!(
            "Monthly usage chart saved in `{}`.",
            out_graph_path.display()
        );
    }

    let mut out_tex_path = path.clone();
    out_tex_path.set_extension("tex");
    write_tex_stats(&out_tex_path, &stats, &path);
    println!("Detailed report saved in `{}`.", out_tex_path.display());
}
