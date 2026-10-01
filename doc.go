// Package cmslens tells which CMS, site builder or web framework a website
// runs, from its HTML and response headers.
//
// Detect works offline on a page you already have. Scan fetches the site
// itself, refusing to connect to private and internal addresses, and can retry
// a site that blocks it through ScraperAPI.
//
// A result names at most one backend engine and at most one presentation
// layer on top of it: a headless site answers [Next.js WordPress], an ordinary
// one [WordPress].
package cmslens
