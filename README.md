**English** | [Русский](README.ru.md)

# cmslens

[![CI](https://github.com/armilenru/cmslens/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/armilenru/cmslens/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/badge/coverage-67.0%25-yellow)](coverage.txt)
[![Go Reference](https://pkg.go.dev/badge/github.com/armilenru/cmslens.svg)](https://pkg.go.dev/github.com/armilenru/cmslens)
[![Release](https://img.shields.io/github/v/release/armilenru/cmslens)](https://github.com/armilenru/cmslens/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

cmslens tells which CMS, site builder or web framework a website runs. It reads
the page's HTML and response headers and matches them against a table of 556
platforms: WordPress, Shopify, Drupal and Webflow, and also many platforms
common on Russian-language sites, such as 1C-Bitrix, Tilda, MODX, InSales,
NetCat, UMI.CMS, uCoz and Flexbe.

It is a Go library and a command-line tool, with no dependencies outside the
standard library.

Try it in the browser: [armilen.ru/en/services/cms](https://www.armilen.ru/en/services/cms)
runs on cmslens.

```text
$ cmslens wordpress.org tilda.cc 1c-bitrix.ru
wordpress.org	WordPress
tilda.cc	Tilda
1c-bitrix.ru	1C-Bitrix
```

## Install

```sh
go install github.com/armilenru/cmslens/cmd/cmslens@latest
```

Ready-made binaries for Linux, macOS and Windows are attached to each
[release](https://github.com/armilenru/cmslens/releases).

## Command line

```text
cmslens [flags] domain...
cmslens -file page.html
```

| Flag | What it does |
|---|---|
| `-json` | Prints one JSON object per site instead of a line of text. |
| `-file PATH` | Detects from a saved HTML page instead of fetching a site. Nothing goes over the network, and no response headers are used. |
| `-timeout DURATION` | Sets the time limit per site, such as `90s` or `2m`. Default: `60s`. |
| `-scraperapi-key KEY` | Retries through [ScraperAPI](https://www.scraperapi.com/) when a site blocks the scanner or does not answer it. Default: the value of `$SCRAPERAPI_KEY`. |
| `-country CODE` | Sets the ScraperAPI country code, such as `us` or `de`, for sites that answer only local visitors. Default: unset, and ScraperAPI picks the country. |
| `-lang VALUE` | Sets the `Accept-Language` header, such as `ru-RU`. Default: `en-US,en;q=0.9`. |
| `-version` | Prints the version and exits. |
| `-h` | Prints the list of flags and exits. |

A flag takes one dash or two: `-json` and `--json` are the same.

Each line of text output is the input, a tab and the answer: the names found,
separated by `, `; `unknown` when no platform scores high enough;
`blocked by an anti-bot page` when the site answered 403, 503 or an anti-bot
page and the second try did not get past it; or `error:` and the reason. With
`-json` the line is an object:

```text
$ cmslens -json wordpress.org
{"input":"wordpress.org","result":{"status":"ok","detected":["WordPress"],"wafBehind":false,"details":{"title":"Blog Tool, Publishing Platform, and CMS \u0026#124; WordPress.org","server":"nginx","statusCode":200}}}
```

The exit code is 0 when every site was read, 1 when a site gave an error or
the file of `-file` could not be read, and 2 when the command line is wrong. A
site that prints `unknown` or `blocked by an anti-bot page` counts as read.

## Library

```go
import "github.com/armilenru/cmslens"

// Fetch the site and detect.
res, err := cmslens.Scan(ctx, "example.com", cmslens.Options{})
fmt.Println(res.Detected) // [Next.js WordPress]

// Or detect from a page you already have.
names := cmslens.Detect(html, resp.Header)
```

`Scan` takes a host name or a URL and fetches the home page of that host: the
path, the query, the port and a leading `www.` are dropped, so
`https://www.example.com/some/page` is fetched as `example.com`. A name in
Cyrillic or another script works too: `президент.рф` is fetched as
`xn--d1abbgf6aiiy.xn--p1ai`. Its result has a `Status`: `ok` when a page was
read, or `cloudflare` when the site answered 403, 503 or an anti-bot page of
any vendor and the second try did not get past it.
`errors.Is(err, cmslens.ErrNoSuchHost)` tells a typo in the name from a site
that did not answer.

## Rust

The repository also holds the detector as a Rust crate, translated from the Go
package and checked against it: on the 2,815 corpus pages that downloaded when
the crate was added, both give the same answers with the same scores, and on
live sites the same JSON lines. The crate is meant to replace the Go module.
Until then the Go package is the reference, and the two tables are kept the
same by a test.

```toml
[dependencies]
cmslens = { git = "https://github.com/armilenru/cmslens", rev = "<commit>" }
```

```rust
let res = cmslens::scan("example.com", &cmslens::Options::default()).await?;
println!("{:?}", res.detected); // ["Next.js", "WordPress"]

let names = cmslens::detect(&body, &headers);
```

`cargo install --git https://github.com/armilenru/cmslens cmslens` builds the
same command, with the same flags and the same output. The crate needs Rust
1.88 or newer.

## How detection works

Each platform has weighted features in the page body (asset paths, generator
tags, inline scripts) and in the response headers (cookies, `x-powered-by`).
A header weighs more than markup, since it is harder to fake. The platform with
the highest score wins, with three rules on top:

- **One site, one engine.** Only one engine wins: a CMS, a site builder or a
  server framework. A presentation layer such as Next.js, Nuxt or Astro is
  reported next to it, so a headless site comes out as two names, such as
  `[Next.js WordPress]`. The name with the higher score comes first, and on a
  tie the engine does.
- **No answer from a single weak hint.** A score below 1 gives no answer.
- **Short fragments start at a word boundary.** Otherwise `evo-` matches inside
  the transliterated `derevo-alyuminievaya`, and a site gets an engine it does
  not run. A test enforces this for every feature.

Every body feature is searched in a single Aho-Corasick pass, and a regular
expression runs only on pages where all its literal pieces were found. Adding
platforms barely changes the cost of a scan.

The comments in [`signatures.go`](signatures.go) explain, for many entries,
which false detection on a real site shaped the feature.

## Safety

`Scan` is meant to run on a server, against addresses typed by users. Every
connection goes through a guard that checks the IP address the socket is about
to open, not just the name: private networks, loopback, link-local and cloud
metadata (`169.254.169.254`) are refused on every redirect and on every
fallback address. Responses are capped at 5 MB, redirects at 5, and proxies
from the environment are ignored.

## Adding a platform

1. Add an entry to `cmsSignatures` in [`signatures.go`](signatures.go). A body
   feature is `` b(`...`) `` (weight 1) or `` bw(`...`, 0.5) ``; a header
   feature sits under the header name in lower case as `` h(`...`) `` (weight
   2) or `hw`. Each is a case-insensitive regular expression in RE2 syntax,
   and a feature made only of letters, digits, `_` and `-` starts with `\b`.
   Prefer features the platform itself emits (its CDN host, generator,
   cookie) over its name, which other sites mention too. On a tie the entry
   higher in the table wins.
2. Add the same entry at the same place to `CMS_SIGNATURES` in
   [`src/signatures.rs`](src/signatures.rs), written like its neighbors:
   `entry("Name", Signature { body: &[b(r"...")], ..NONE })`. The test
   `table_matches_the_go_source` fails until both tables hold the same
   features with the same weights.
3. Raise the number of entries by one where it is written down: in
   `TestAllSignaturesCompile` ([`signatures_test.go`](signatures_test.go)), in
   `all_signatures_compile`
   ([`src/tests/signatures_test.rs`](src/tests/signatures_test.rs)) and in
   the first paragraph of both READMEs. `plans_follow_the_features`
   ([`src/tests/match_test.rs`](src/tests/match_test.rs)) counts the
   features by kind: its failure prints the new counts, and they should move
   by the features you added.
4. If the platform is a presentation layer that runs on top of an engine,
   like Next.js, add its name to `frontendLayer` in
   [`signatures.go`](signatures.go) and to `FRONTEND_LAYER` in
   [`src/signatures.rs`](src/signatures.rs).
5. If a real site was detected wrongly, add the trimmed markup as a test to
   [`falsepositive_test.go`](falsepositive_test.go) and the same test, named
   in snake case, to
   [`src/tests/falsepositive_test.rs`](src/tests/falsepositive_test.rs).
6. Add a public site that runs the platform to
   [`testdata/corpus.tsv`](testdata/corpus.tsv): the domain, a tab and the
   expected answer. Then check the whole table against live sites:

   ```sh
   testdata/corpus.sh /tmp/corpus
   CMSLENS_CORPUS=/tmp/corpus go test -count=1 -run TestCorpus -v .
   CMSLENS_CORPUS=/tmp/corpus cargo test corpus -- --ignored
   ```

   The script needs bash and curl. It downloads about 2,900 public sites,
   about 1,900 of them with an expected answer; the rest are fetched and
   printed, but not compared. `testdata/corpus.sh /tmp/corpus example.com`
   downloads only the sites you name. Sites answer differently depending on
   where you ask from, so a few mismatches can come from the network rather
   than the table.

## Contributing

Every change starts from an issue. A bug report says what you ran, what you
expected and what you got; a missing platform or something the tool does not
do yet is reported the same way. A pull request names its issue and does one
thing, and a bug fix comes with a test that fails without it. The main branch
takes changes only through pull requests, and GitHub lets one be merged only
when the Go tests, the linter and the coverage check pass; the Rust checks run
on every pull request too.

You need Go 1.24 or newer, golangci-lint 2 (CI runs v2.15.0), and Rust 1.88 or
newer with clippy and rustfmt. Before you push:

```sh
go test -race ./...
golangci-lint run         # every linter but the few .golangci.yml turns off
go run ./internal/cover   # rewrites coverage.txt and the badge in both READMEs
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Commit `coverage.txt` and the two READMEs with the change. `coverage.txt`
counts the statements without a test in each file, and CI fails when the
committed copy differs from what the tests give: a change that leaves code
untested shows that in its own diff.

## Status

cmslens is maintained by [armilen](https://github.com/armilenru) and runs the
detector on [armilen.ru](https://www.armilen.ru/en/services/cms). It is at
0.x: the library API may still change between minor versions, and a change
that breaks callers is listed first in the release notes. A release is a tag:
pushing `v0.2.0` runs CI, then builds binaries for six platforms and
publishes them with notes made from the merged pull requests.

## License

[MIT](LICENSE)
