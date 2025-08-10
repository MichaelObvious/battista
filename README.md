# [Battista](https://en.wikipedia.org/wiki/List_of_Donald_Duck_universe_characters#Albert_Quackmore)

It keeps track of expenses and processes the data to provide some statistics.

## Usage

### Without Installation

```sh
cargo run --release <path/to/file.csv>
```

### With Installation

```sh
cargo install --path .
battista <path/to/file.csv>
```

Supply a `.csv` file like the [`example.csv`](./example.csv).

Spending categories as of right now include: `Books`, `Charity`, `Clothing`, `Grocery`, `Education`, `Entrateinment`, `Fine`, `Gift`, `Healthcare`, `Hobby`, `Insurance`, `Rent`, `Restaurants`, `Savings`, `Shopping`, `Sport`, `Taxes`, `Transportation`, `Travel`, `Utilities`, `Miscellaneous`, `Unknown`.
