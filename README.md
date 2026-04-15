# [Battista](https://en.wikipedia.org/wiki/List_of_Donald_Duck_universe_characters#Albert_Quackmore)

It keeps track of expenses and processes the data to provide some statistics.

## Usage

### Building & Running

Without Installation:

```sh
cargo run --release <path/to/file.csv>
```

With Installation:

```sh
cargo install --path .
battista <path/to/file.xml>
```

### Adding expenses to a file

```sh
battista add <path/to/file.xml>
```

## Example

To see an example of an `.xml` file with its report, run `python3 ./build_example.py`.
