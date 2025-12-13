# editex

A TeX-inspired scientific document editor built with GTK4 and Skia.

## Features

- Rich text editing with customizable attributes (bold, italic, font size, etc.)
- Real-time rendering using Skia graphics
- JSON-based document format with support for pages and styled text spans
- Keyboard navigation and text editing (arrow keys, backspace, delete, enter)
- Visual cursor positioning with mouse click support
- Configurable window settings

## Building

```bash
cargo build --release
```

## Usage

Launch the editor and open a document:

```bash
./editex --document path/to/document.json
```

Or launch without a document and use the toolbar button to open one.

## Document Format

Documents are stored as JSON files with the following structure:

```json
{
  "topMargin": 50.0,
  "bottomMargin": 50.0,
  "leftMargin": 50.0,
  "rightMargin": 50.0,
  "fontSize": 16.0,
  "elements": [
    {
      "anchorPoint": [100.0, 100.0],
      "spans": [
        ["Hello ", {}],
        ["World", {}]
      ]
    }
  ]
}
```

## Configuration

Configuration is stored in `~/.editex/config.json` and includes window dimensions and other settings.
