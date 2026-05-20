# [Battista](https://en.wikipedia.org/wiki/List_of_Donald_Duck_universe_characters#Albert_Quackmore)

A personal finance tracker that reads expense data from an XML file and generates a compiled PDF spending report.

## Data Format

Transactions are stored in a plain XML file with three element types:

```xml
<budget amount="1500" duration="30" date="01/01/2025"/>
<budget category="Food" amount="400" duration="30" date="01/01/2025"/>
<transaction amount="12.50" category="Food" date="15/05/2025" payment-method="card" note="Lunch"/>
<extra amount="50" date="20/05/2025" payment-method="cash" note="Gift from friend"/>
```

- `<budget>`: sets a daily spending allowance (`amount / duration`), optionally per category.
- `<transaction>`: records an expense.
- `<extra>`: adds a one-off budget boost for a specific day.

## Usage

```sh
# Generate a PDF report
battista path/to/file.xml

# Interactively add transactions, then generate a report
battista add path/to/file.xml
```

### Output Report

Writes a [Typst](https://typst.app) source file (`.typ`) and compiles it to PDF via `typst compile`. The PDF includes:

- monthly budget breakdown by category;
- a recovery plan if overspent;
- an spending timeline chart (last 365 days + 30-day prediction);
- bar charts for the last 5 years, 12 months, and 12 weeks;
- category spending tables for each time window.

### Interactive Mode

Running with `add` drops into a prompt-driven loop. It pre-fills defaults from the most recent transaction (date, category, payment method) and saves each entry immediately to the XML file, with a `.bak` backup created first.

### Requirements

- [`typst`](https://typst.app): must be installed and on `$PATH` for PDF compilation.
