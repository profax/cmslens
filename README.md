# cmslens

cmslens tells which CMS, site builder or web framework a website runs. It reads
the page's HTML and response headers and matches them against a table of 556
platforms: WordPress, Shopify, Drupal and Webflow, and also many platforms
common on Russian-language sites, such as 1C-Bitrix, Tilda, MODX, InSales,
NetCat, UMI.CMS, uCoz and Flexbe.

It is a Go library and a command-line tool, with no dependencies outside the
standard library.

Try it in the browser: [armilen.ru/en/services/cms](https://www.armilen.ru/en/services/cms)
runs on cmslens.

```
$ cmslens wordpress.org tilda.cc 1c-bitrix.ru
wordpress.org	WordPress
tilda.cc	Tilda
1c-bitrix.ru	1C-Bitrix
```

## Install

```
go install github.com/profax/cmslens/cmd/cmslens@latest
```

Ready-made binaries for Linux, macOS and Windows are attached to each
[release](https://github.com/profax/cmslens/releases).

## Command line

```
cmslens [flags] domain...
cmslens -file page.html
```

| Flag | Meaning |
|---|---|
| `-json` | one JSON object per site instead of text |
| `-file page.html` | detect from a saved page, without network access |
| `-timeout 60s` | time limit per site |
| `-scraperapi-key KEY` | retry sites that block the scanner through [ScraperAPI](https://www.scraperapi.com/) (also read from `$SCRAPERAPI_KEY`) |
| `-country us` | ScraperAPI country code, for sites that answer only local visitors |
| `-lang ru-RU` | `Accept-Language` to send |

The exit code is 1 when any site could not be scanned.

## Library

```go
import "github.com/profax/cmslens"

// Fetch the site and detect.
res, err := cmslens.Scan(ctx, "https://example.com/some/page", cmslens.Options{})
fmt.Println(res.Detected) // [Next.js WordPress]

// Or detect from a page you already have.
names := cmslens.Detect(html, resp.Header)
```

`Scan` accepts a URL or a bare host. Its result has a `Status`: `ok`, or
`cloudflare` when an anti-bot page stood in the way and could not be passed.
`errors.Is(err, cmslens.ErrNoSuchHost)` tells a typo in the name from a site
that did not answer.

## How detection works

Each platform has weighted features in the page body (asset paths, generator
tags, inline scripts) and in the response headers (cookies, `x-powered-by`).
A header weighs more than markup, since it is harder to fake. The platform with
the highest score wins, with three rules on top:

- **One site, one engine.** Only one backend wins. A presentation layer such
  as Next.js, Nuxt or Astro is reported next to it, so a headless site comes
  out as `[Next.js WordPress]`.
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

1. Add an entry to `cmsSignatures` in [`signatures.go`](signatures.go). Prefer
   features the platform itself emits (its CDN host, generator, cookie) over
   its name, which other sites mention too.
2. If a real site was detected wrongly, add the trimmed markup as a test in
   [`falsepositive_test.go`](falsepositive_test.go).
3. Check the whole table against live sites:

   ```
   testdata/corpus.sh /tmp/corpus
   CMSLENS_CORPUS=/tmp/corpus go test -count=1 -run TestCorpus -v .
   ```

   [`testdata/corpus.tsv`](testdata/corpus.tsv) lists about 2,900 public sites
   with their expected answers. Sites answer differently depending on where
   you ask from, so a few mismatches can come from the network rather than the
   table.

## License

[MIT](LICENSE)
