# Operator

**Operator is a web server.** You provide [a
directory](samples/realistic-advanced) and Operator serves it over HTTP.

It serves static files the way you'd expect, but it can also serve dynamic
content that is generated at request time by [handlebars
templates](samples/realistic-advanced/home.html.hbs) or
[executables](samples/realistic-advanced/play-lottery.html.sh).

More information is available on [the Operator
website](http://operator.mattkantor.com).

## Installation

Operator is a single self-contained binary. You can download a build from [the
releases list](https://github.com/mkantor/operator/releases), unzip it, and run
it from any working directory.

## Usage

The CLI has three subcommands:

1. `eval` evaluates a handlebars template from STDIN.
1. `get` renders content from a content directory.
1. `serve` starts an HTTP server.

`serve` is where the real action is, but the other two come in handy at times.

These commands all require a _content directory_, which is just the folder
where your website lives. There are a bunch of sample content directories in
[`samples/`](samples).

To learn more, run `operator --help` or `operator <SUBCOMMAND> --help`.

### Example

Let's start a server for [one of the samples](samples/realistic-advanced):

```sh
operator -vv serve \
  --content-directory=samples/realistic-advanced \
  --index-route=/home \
  --error-handler-route=/error-handler \
  --bind-to=127.0.0.1:8080
```

Then open [http://localhost:8080](http://localhost:8080) in your browser of
choice.

## Disclaimer

Operator is very young and has not been battle-hardened. There are known flaws
and obvious missing features that need to be addressed. The major ones are
filed as [issues](https://github.com/mkantor/operator/issues). All feedback is
greatly appreciated.

This is my first nontrivial Rust project and I'm sure there are places where
things could be improved. One of the reasons I created Operator was to get more
experience with the language, so if you notice anything iffy (no matter how
small), please [open an issue](https://github.com/mkantor/operator/issues/new)
to help me out! ❤️

---

[![Operator](operator.jpg)](https://operator.mattkantor.com)
