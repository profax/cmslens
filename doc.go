// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

// Package cmslens tells which CMS, site builder or web framework a website
// runs, from its HTML and response headers.
//
// Detect works offline on a page you already have. Scan keeps only the host of
// the name or URL it is given and fetches the home page of that host itself,
// refusing to connect to private and internal addresses. When a site blocks it,
// Scan can retry through ScraperAPI.
//
// A result names at most one backend engine and at most one presentation
// layer on top of it: a headless site answers two names, such as
// [Next.js WordPress], an ordinary one [WordPress]. The name with the higher
// score comes first; on a tie the engine does.
package cmslens
