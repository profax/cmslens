// SPDX-FileCopyrightText: 2026 Oleg Smirnov
// SPDX-License-Identifier: MIT

use std::sync::{LazyLock, OnceLock};

use regex::Regex;

use crate::re2;

/// One feature: a regular expression in RE2 syntax and the weight of a match.
/// Weights are not always whole (October/Winter CMS use 0.5), so scores are
/// `f64`; every weight is a multiple of one half, so a sum does not depend on
/// the order it is taken in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SigItem {
    pub(crate) pattern: &'static str,
    pub(crate) weight: f64,
}

/// The set of features of one CMS or framework: in the page body and in the
/// response headers. A header weighs more by default, because it is harder to
/// fake than markup. Header names are lower case.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Signature {
    pub(crate) body: &'static [SigItem],
    pub(crate) headers: &'static [(&'static str, &'static [SigItem])],
}

const BODY_DEFAULT_WEIGHT: f64 = 1.0;
const HEADER_DEFAULT_WEIGHT: f64 = 2.0;

// A short word-like feature is written with a leading `\b`. Without the boundary
// the fragment matches at the end of someone else's word, and on Russian sites
// that is everyday life rather than a rare case: transliteration turns
// `derevo-alyuminievaya` into a match for `evo-`, and someone's path
// `WEEKEND_Online_Site/` into a match for `_site/`. No boundary on the right: what
// follows there is almost always meaningful (`hubspotusercontent`,
// `bx-core-window`). Every expression is case-insensitive, see `compile`.
const fn b(pattern: &'static str) -> SigItem {
    SigItem {
        pattern,
        weight: BODY_DEFAULT_WEIGHT,
    }
}
const fn bw(pattern: &'static str, weight: f64) -> SigItem {
    SigItem { pattern, weight }
}
const fn h(pattern: &'static str) -> SigItem {
    SigItem {
        pattern,
        weight: HEADER_DEFAULT_WEIGHT,
    }
}
const fn hw(pattern: &'static str, weight: f64) -> SigItem {
    SigItem { pattern, weight }
}

/// Keeps the name next to the signature: the order of the table decides who
/// wins a tie (`select_winners` sorts stably).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CmsEntry {
    pub(crate) name: &'static str,
    pub(crate) sig: Signature,
}

const fn entry(name: &'static str, sig: Signature) -> CmsEntry {
    CmsEntry { name, sig }
}

/// The missing half of a signature with only body or only header features.
const NONE: Signature = Signature {
    body: &[],
    headers: &[],
};

/// A feature ready to match: the expression as Go wrote it (with its `(?i)`,
/// which the index plans from) and the expression `regex` runs. The expression
/// is built on its first use: most body features are decided by the automaton
/// alone and never need it, and a built expression keeps its program and
/// search caches for the life of the process.
#[derive(Debug)]
pub(crate) struct Feature {
    pub(crate) go: String,
    re: OnceLock<Regex>,
    pub(crate) weight: f64,
}

impl Feature {
    /// A feature that does not compile is a broken table, not a bad page, so
    /// it panics like `regexp.MustCompile`; `all_signatures_compile` builds
    /// every one to catch that before a release.
    pub(crate) fn re(&self) -> &Regex {
        self.re.get_or_init(|| {
            Regex::new(&re2::translate(&self.go))
                .unwrap_or_else(|e| panic!("feature {:?} does not compile: {e}", self.go))
        })
    }

    #[cfg(test)]
    pub(crate) fn is_built(&self) -> bool {
        self.re.get().is_some()
    }
}

#[derive(Debug)]
pub(crate) struct CompiledEntry {
    pub(crate) name: &'static str,
    pub(crate) body: Vec<Feature>,
    pub(crate) headers: Vec<(&'static str, Vec<Feature>)>,
}

fn compile(item: &SigItem) -> Feature {
    Feature {
        go: format!("(?i){}", item.pattern),
        re: OnceLock::new(),
        weight: item.weight,
    }
}

/// The table in table order; each expression is built on first use, see
/// `Feature::re`.
pub(crate) static COMPILED: LazyLock<Vec<CompiledEntry>> = LazyLock::new(compile_table);

pub(crate) fn compile_table() -> Vec<CompiledEntry> {
    CMS_SIGNATURES
        .iter()
        .map(|e| CompiledEntry {
            name: e.name,
            body: e.sig.body.iter().map(compile).collect(),
            headers: e
                .sig
                .headers
                .iter()
                .map(|(name, items)| (*name, items.iter().map(compile).collect()))
                .collect(),
        })
        .collect()
}

#[rustfmt::skip]
pub(crate) static CMS_SIGNATURES: &[CmsEntry] = &[
    entry("1C-Bitrix", Signature {
        body: &[b(r"bitrix/js/"), b(r"bitrix/templates/"), b(r"bitrix/css/"), b(r"BX\.message"), b(r"\bbx-core"), b(r"\bbx-panel"), b(r"/upload/uf/")],
        headers: &[
            ("set-cookie", &[h(r"BITRIX_SM_")]),
            ("x-powered-cms", &[h(r"Bitrix Site Manager")]),
        ],
    }),
    entry("WordPress", Signature {
        body: &[b(r"/wp-content/"), b(r"/wp-includes/"), b(r"\bwp-submit"), b(r"wp-embed\.min\.js"), b(r"<meta[^>]+generator[^>]+WordPress")],
        headers: &[
            // The `x-pingback` header alone is not WordPress: pingback is a general blog
            // protocol, and Typecho announces it with its own `/action/xmlrpc`
            ("x-pingback", &[h(r"xmlrpc\.php")]),
            ("link", &[h(r"/wp-json/")]),
            ("set-cookie", &[h(r"wp-settings"), h(r"wordpress_")]),
        ],
    }),
    // `tilda.ws` is not a feature: it is the address of any Tilda project, and
    // links to it sit in the menus of other sites (3lp.me on Eleventy). From the
    // CDN only scripts and styles count: images from it are embedded in articles
    // on other engines too (promo-blog.ru on Next.js)
    entry("Tilda", Signature {
        body: &[b(r"tildacdn\.(com|re)/(js|css)/"), b(r"tilda3\.0\.min\.js"), b(r"tilda-grid\.css"), b(r#"id="allrecords""#), b(r"<meta[^>]+generator[^>]+Tilda")],
        ..NONE
    }),
    // The `assets/components/` directory counts only from the site root (MODX
    // writes it relative or from `/`): a nested path occurs in third-party plugins
    // and CDNs (`shared-assets/components/` at THG), and a root path on a foreign
    // host at Envato
    entry("MODX", Signature {
        body: &[b(r#"["'(=]/?assets/components/"#), b(r"assets/templates/"), b(r"\bMODX_MEDIA"), b(r"\[\[\+")],
        ..NONE
    }),
    // `evo-` cannot stand here without a word boundary: the fragment matches at
    // the end of someone else's word, and transliteration ("derevo-alyuminievaya",
    // "dizanalevo-m") declared Evolution CMS on sites that do not run it at all.
    // It is recognised by what belongs to the CMS itself: the snippets directory
    // (MODX Revolution does not have it) and the engine's name in the markup
    entry("Evolution CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Evolution CMS"), b(r"assets/templates/"), b(r"assets/snippets/"), b(r"\bevolutioncms")],
        headers: &[
            ("set-cookie", &[h(r"\bevo[a-zA-Z0-9]{5,}")]),
        ],
    }),
    entry("OpenCart", Signature {
        body: &[b(r"catalog/view/javascript/"), b(r"catalog/view/theme/"), b(r"index\.php\?route=checkout/")],
        headers: &[
            ("set-cookie", &[h(r"OCSESSID")]),
        ],
    }),
    entry("Joomla", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Joomla"), b(r"/media/system/js/"), b(r"/media/jui/")],
        ..NONE
    }),
    entry("Drupal", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Drupal"), b(r"/sites/default/files/"), b(r"Drupal\.settings"), b(r"\bdata-drupal-selector")],
        ..NONE
    }),
    // A bare `cdn.shopify.com` is not a feature: www.framer.com loads a consent
    // script from it too. Store files live under `/s/files/` or are served from
    // the store's own domain under `/cdn/shop/`
    entry("Shopify", Signature {
        body: &[b(r"cdn\.shopify\.com/s/files/"), b(r"/cdn/shop/"), b(r"Shopify\.shop"), b(r"Shopify\.theme"), b(r"\bshopify-section")],
        headers: &[
            ("x-shopid", &[h(r".+")]),
            ("powered-by", &[h(r"Shopify")]),
        ],
    }),
    entry("Wix", Signature {
        body: &[b(r"static\.wixstatic\.com"), b(r"<meta[^>]+generator[^>]+Wix\.com")],
        ..NONE
    }),
    entry("Webflow", Signature {
        body: &[b(r"\bdata-wf-page"), b(r"\bdata-wf-site"), b(r"webflow\.js")],
        ..NONE
    }),
    entry("Squarespace", Signature {
        body: &[b(r"static1\.squarespace\.com"), b(r"Squarespace\.VERSION")],
        ..NONE
    }),
    entry("Magento", Signature {
        body: &[b(r"text/x-magento-init"), b(r"Mage\.Cookies")],
        ..NONE
    }),
    // The word PrestaShop itself is not a feature: hosting providers mention it in
    // their one-click install ads (gandi.net)
    entry("PrestaShop", Signature {
        body: &[bw(r"var prestashop\s*=", 2.0)],
        headers: &[
            ("set-cookie", &[h(r"PrestaShop")]),
        ],
    }),
    entry("InSales", Signature {
        body: &[b(r"\binsales\.ru"), b(r"\bstatic-insales")],
        ..NONE
    }),
    entry("DataLife Engine", Signature {
        body: &[b(r"engine/classes/js/"), b(r"<meta[^>]+generator[^>]+DataLife Engine"), b(r"\bdle_root"), b(r"\bdle_admin")],
        headers: &[
            ("set-cookie", &[h(r"dle_user_id"), h(r"dle_password"), h(r"dle_hash")]),
        ],
    }),
    entry("UMI.CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+UMI\.CMS"), b(r"/js/client/umi"), b(r"<meta[^>]+author[^>]+Umisoft"), b(r"xmlns:umi"), b(r"umi\.ru"), b(r"umi-cms\.ru")],
        ..NONE
    }),
    entry("NetCat", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+NetCat"), b(r"\bnetcat_files"), bw(r"/netcat/", 3.0)],
        ..NONE
    }),
    entry("CS-Cart", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+CS-Cart"), b(r"\bcm-dialog-auto-size"), b(r"Tygh\.")],
        ..NONE
    }),
    entry("HostCMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+HostCMS"), b(r"\bhostcms_files"), b(r"\bhostcms")],
        ..NONE
    }),
    entry("Amiro.CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Amiro"), b(r"\bamiro\.ru")],
        ..NONE
    }),
    entry("DIAFAN.CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+DIAFAN\.CMS"), b(r"\bdiafan")],
        ..NONE
    }),
    entry("Moguta.CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Moguta\.CMS"), b(r"\bmg-core")],
        ..NONE
    }),
    entry("Shop-Script", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Shop-Script"), b(r"\bwa-data"), b(r"\bwa-content")],
        ..NONE
    }),
    entry("VamShop", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+VamShop"), b(r"\bvamshop")],
        ..NONE
    }),
    // A SaaS platform for restaurants and delivery: the platform builds the site
    // and Next.js renders it. Recognised by its CDN and API domains, which sit on
    // every client page and nowhere else. A bare `starterapp.ru` weighs less:
    // sites that merely link to the platform write it too
    entry("StarterApp", Signature {
        body: &[bw(r"cdn\.starterapp\.(ru|co)", 3.0), bw(r"api\.starterapp\.ru", 3.0), b(r"\bstarterapp\.ru")],
        ..NONE
    }),
    // The word `ucoz` is not a feature: links to an old uCoz site appear on other
    // people's pages too (bolknote.ru)
    entry("uCoz", Signature {
        body: &[b(r#"="/\.s/src/"#), b(r"\.ucoz\.net/"), b(r"\buid\.me")],
        headers: &[
            ("set-cookie", &[h(r"uCoz=")]),
        ],
    }),
    entry("uKit", Signature {
        body: &[b(r"\bukit\.com"), b(r"<meta[^>]+generator[^>]+uKit")],
        ..NONE
    }),
    // A right-hand boundary is needed too: `setup.ru` is the start of the
    // unrelated domain `setup.runtipi.io`, and a site on Nextra was declared a
    // Russian site builder
    entry("Setup.ru", Signature {
        body: &[b(r"\bsetup\.ru\b")],
        ..NONE
    }),
    entry("Nethouse", Signature {
        body: &[b(r"\bnethouse\.ru")],
        ..NONE
    }),
    entry("AdvantShop", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+AdvantShop"), b(r"\badvantshop")],
        ..NONE
    }),
    entry("Ghost", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Ghost"), b(r"\bghost-frontend")],
        ..NONE
    }),
    entry("October CMS", Signature {
        body: &[bw(r"/themes/[^/]+/assets/", 0.5), b(r"/modules/system/assets/"), b(r"/modules/system/assets/js/framework(?:\.combined)?\.js"), b(r"/plugins/[a-z0-9_-]+/[a-z0-9_-]+/assets/"), bw(r"/combine/[a-f0-9]+", 2.0), b(r"<meta[^>]+generator[^>]+October")],
        headers: &[
            ("set-cookie", &[h(r"october_session")]),
        ],
    }),
    entry("Winter CMS", Signature {
        body: &[bw(r"/themes/[^/]+/assets/", 0.5), b(r"/modules/system/assets/"), b(r"/modules/system/assets/js/framework(?:\.combined)?\.js"), b(r"/plugins/[a-z0-9_-]+/[a-z0-9_-]+/assets/"), bw(r"/combine/[a-f0-9]+", 2.0), b(r"<meta[^>]+generator[^>]+Winter")],
        headers: &[
            ("set-cookie", &[h(r"winter_session")]),
        ],
    }),
    // HubSpot's tracker and forms are put on sites of every engine, so neither
    // `js.hs-scripts.com` nor the word itself is a feature: the feature is what
    // its CMS emits
    entry("HubSpot", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+HubSpot"), bw(r"/hub_generated/", 2.0)],
        headers: &[
            ("x-hs-content-id", &[h(r".+")]),
        ],
    }),
    entry("Next.js", Signature {
        body: &[b(r#"id="__NEXT_DATA__""#), b(r"/_next/static/"), b(r"<meta[^>]+generator[^>]+Next\.js")],
        ..NONE
    }),
    entry("Nuxt.js", Signature {
        body: &[b(r#"id="__NUXT__""#), b(r"window\.__NUXT__"), b(r"\bnuxt-link"), b(r"\bdata-n-head")],
        headers: &[
            ("x-powered-by", &[h(r"\bNuxt\b")]),
        ],
    }),
    entry("Astro", Signature {
        body: &[b(r"<astro-island"), b(r#"class="astro-[a-zA-Z0-9]{8}""#), b(r"<meta[^>]+generator[^>]+Astro")],
        ..NONE
    }),
    // `<meta name="csrf-token">` is written by Rails, Yii, InstantCMS and
    // hand-made backends alike, so it does not name Laravel: on www.rbc.ru it was
    // the only feature
    entry("Laravel", Signature {
        body: &[b(r#"name="_token""#)],
        headers: &[
            ("set-cookie", &[h(r"XSRF-TOKEN"), h(r"laravel_session")]),
        ],
    }),
    entry("Django", Signature {
        body: &[b(r#"name="csrfmiddlewaretoken""#), b(r"\bdjango-admin")],
        headers: &[
            ("set-cookie", &[h(r"csrftoken")]),
        ],
    }),
    entry("Hugo", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Hugo"), b(r"powered by Hugo")],
        ..NONE
    }),
    // `_site/` was removed: it is Jekyll's build directory and never appears in
    // served URLs, but it matched inside someone else's path
    // (`WEEKEND_Online_Site/`)
    entry("Jekyll", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Jekyll"), b(r"Jekyll v")],
        ..NONE
    }),
    entry("Gatsby", Signature {
        body: &[b(r#"id="___gatsby""#), b(r"\bgatsby-image"), b(r"<meta[^>]+generator[^>]+Gatsby")],
        ..NONE
    }),
    entry("Svelte", Signature {
        body: &[b(r#"class="svelte-[a-zA-Z0-9]{5,8}""#), b(r"\b__sveltekit")],
        ..NONE
    }),
    entry("Webasyst", Signature {
        body: &[bw(r"\bwa-data/", 3.0), bw(r"\bwa-content/", 3.0), b(r"<meta[^>]+generator[^>]+Webasyst")],
        ..NONE
    }),
    entry("Simpla", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Simpla")],
        ..NONE
    }),
    entry("OkayCMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+OkayCMS")],
        ..NONE
    }),
    entry("ABO.CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+ABO\.CMS")],
        ..NONE
    }),
    entry("SiteEdit", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+SiteEdit")],
        ..NONE
    }),
    entry("ImageCMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+ImageCMS")],
        ..NONE
    }),
    entry("LPgenerator", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+LPgenerator")],
        ..NONE
    }),
    entry("Flexbe", Signature {
        body: &[b(r"\bflexbe\.ru"), b(r"\bflexbe\.com"), b(r#"<meta[^>]+name="flexbe-theme-id""#), b(r"window\.flexbe_cli")],
        ..NONE
    }),
    entry("Mottor", Signature {
        body: &[b(r"\bmottor"), b(r"\blptrend")],
        ..NONE
    }),
    entry("Craftum", Signature {
        body: &[b(r"\bcraftum\.com"), b(r"\bcraftum")],
        ..NONE
    }),
    entry("Tobiz", Signature {
        body: &[b(r"\btobiz\.net")],
        ..NONE
    }),
    entry("Creatium", Signature {
        body: &[b(r"\bcreatium\.io"), b(r"assets\.creatium\.io")],
        ..NONE
    }),
    entry("CMS.S3", Signature {
        body: &[b(r"\bcms\.s3"), b(r"\bmegagroup\.ru"), b(r#"="/g/s3/"#), b(r#"="/t/v[0-9]+/"#), b(r"/g/shop2"), b(r"/shared/s3/")],
        ..NONE
    }),
    // Taptop is built on the Megagroup platform and carries the same paths and
    // tracker as CMS.S3 (up to 3 points), so its generator weighs more
    entry("Taptop", Signature {
        body: &[bw(r#"<meta[^>]+content="Taptop"[^>]+name="generator""#, 5.0)],
        ..NONE
    }),
    entry("Typo3", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+TYPO3"), b(r"\btypo3temp/"), b(r"\btypo3conf/")],
        ..NONE
    }),
    // Since version 9 the engine is called Concrete CMS; the table keeps the old
    // name, which users know better
    entry("Concrete5", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+concrete( |5)")],
        headers: &[
            ("set-cookie", &[h(r"\bCONCRETE5=")]),
        ],
    }),
    entry("Contao", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Contao")],
        ..NONE
    }),
    entry("Craft CMS", Signature {
        headers: &[
            ("x-powered-by", &[h(r"Craft CMS")]),
        ],
        ..NONE
    }),
    entry("Weebly", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Weebly")],
        ..NONE
    }),
    // Links to blogger.com appear in the share buttons of other sites too: the
    // features are resources served by Blogger itself and its generator written
    // value first
    entry("Blogger", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Blogger"), b(r#"content=['"]blogger['"] name=['"]generator"#), b(r"\bwww\.blogger\.com/(?:static|dyn-css|openid-server)"), b(r"\b(?:resources|widgets)\.blogblog\.com/")],
        ..NONE
    }),
    entry("phpBB", Signature {
        body: &[b(r"<p[^>]+>Powered by.+phpBB")],
        ..NONE
    }),
    // The engine's name in text is not a feature: the Discourse forum writes it in
    // articles about migrating from XenForo
    entry("XenForo", Signature {
        body: &[b(r"js/xenforo/"), bw(r"\bdata-xf-init", 2.0), b(r#"<html[^>]+id="XF""#), b(r"/js/xf/")],
        headers: &[
            ("set-cookie", &[h(r"\bxf_csrf")]),
        ],
    }),
    entry("vBulletin", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+vBulletin")],
        ..NONE
    }),
    entry("BigCommerce", Signature {
        body: &[b(r"cdn\d+\.bigcommerce\.com")],
        ..NONE
    }),
    entry("Volusion", Signature {
        body: &[b(r"\bvolusion\.com")],
        ..NONE
    }),
    entry("Umbraco", Signature {
        headers: &[
            ("x-umbraco-version", &[h(r".+")]),
        ],
        ..NONE
    }),
    entry("Strapi", Signature {
        headers: &[
            ("x-powered-by", &[h(r"Strapi")]),
        ],
        ..NONE
    }),
    // A bare `sanity.io` is not a feature: the name appears in the list of data
    // sources on gridsome.org. A site on Sanity pulls its content from Sanity's
    // hosts
    entry("Sanity", Signature {
        body: &[b(r"\b(?:cdn|apicdn)\.sanity\.io"), b(r"\.api\.sanity\.io")],
        ..NONE
    }),
    entry("Contentful", Signature {
        body: &[b(r"\bcontentful\.com")],
        ..NONE
    }),
    entry("Prismic", Signature {
        body: &[b(r"\bprismic\.io")],
        ..NONE
    }),
    entry("Jimdo", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Jimdo")],
        ..NONE
    }),
    entry("Yola", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Yola")],
        ..NONE
    }),
    entry("Strikingly", Signature {
        body: &[b(r"\bstrikingly\.com")],
        ..NONE
    }),
    entry("Duda", Signature {
        body: &[b(r"\bduda\.co")],
        ..NONE
    }),
    entry("Webnode", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Webnode")],
        ..NONE
    }),
    entry("Sitecore", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Sitecore")],
        ..NONE
    }),
    entry("Kentico", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Kentico")],
        ..NONE
    }),
    entry("Liferay", Signature {
        headers: &[
            ("liferay-portal", &[h(r".+")]),
        ],
        ..NONE
    }),
    entry("Zen Cart", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Zen Cart")],
        ..NONE
    }),
    entry("osCommerce", Signature {
        body: &[b(r"\boscommerce")],
        ..NONE
    }),
    entry("ExpressionEngine", Signature {
        headers: &[
            ("set-cookie", &[h(r"exp_last_visit")]),
        ],
        ..NONE
    }),
    // `/content/dam/` is AEM's file store, but other sites link to it too, while
    // client libraries under `/etc.clientlibs/` are served only by AEM itself.
    // Edge Delivery Services (formerly Franklin) is another way of serving the
    // same AEM: a page built from a document and two scripts of its own instead of
    // clientlibs
    entry("Adobe Experience Manager", Signature {
        body: &[bw(r"/etc\.clientlibs/", 2.0), b(r"/content/dam/"), b(r"/scripts/(?:aem|lib-franklin)\.js")],
        ..NONE
    }),
    entry("Salesforce Commerce Cloud", Signature {
        body: &[bw(r"/on/demandware\.static/", 2.0), b(r"/on/demandware\.store/")],
        headers: &[
            ("set-cookie", &[h(r"\bdwsid="), h(r"\bdwanonymous_")]),
        ],
    }),
    entry("Framer", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Framer"), b(r"framerusercontent\.com"), b(r"\bdata-framer-")],
        headers: &[
            ("server", &[h(r"^Framer\b")]),
        ],
    }),
    entry("Weblium", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Weblium"), bw(r"res2\.weblium\.site", 2.0)],
        ..NONE
    }),
    entry("Discourse", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Discourse"), bw(r"\bdata-discourse-setup", 2.0), b(r"discourse-cdn\.com")],
        ..NONE
    }),
    entry("MediaWiki", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+MediaWiki"), b(r"/load\.php\?lang="), b(r"\bRLCONF")],
        ..NONE
    }),
    entry("Moodle", Signature {
        body: &[b(r"/theme/yui_combo\.php"), b(r"/theme/image\.php")],
        headers: &[
            ("set-cookie", &[h(r"\bMoodleSession")]),
        ],
    }),
    entry("Plone", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Plone"), b(r"\+\+plone\+\+"), b(r"\+\+resource\+\+")],
        ..NONE
    }),
    entry("PHPShop", Signature {
        body: &[bw(r"/phpshop/templates/", 2.0)],
        headers: &[
            ("x-powered-by", &[h(r"PHPShop")]),
        ],
    }),
    entry("PlatformaLP", Signature {
        body: &[bw(r"\bdata-plp-analytics", 2.0), b(r"/widgets/plp-analytics"), b(r"\bplp-field")],
        ..NONE
    }),
    entry("ReadyScript", Signature {
        body: &[b(r"/storage/system/resized/"), b(r#"/templates/[^/"]+/resource/"#), b(r"/cache/resource/min_")],
        ..NONE
    }),
    entry("InstantCMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+InstantCMS")],
        headers: &[
            ("x-powered-by", &[h(r"InstantCMS")]),
            ("set-cookie", &[h(r"\bInstantCMS\[")]),
        ],
    }),
    entry("StoreLand", Signature {
        body: &[b(r"\.storeland\.ru/")],
        headers: &[
            ("x-generator", &[h(r"StoreLand")]),
        ],
    }),
    entry("Mobirise", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Mobirise"), bw(r"assets/mobirise/", 2.0)],
        ..NONE
    }),
    entry("GoDaddy Website Builder", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Go Daddy Website Builder"), bw(r"img1\.wsimg\.com/isteam/", 2.0)],
        ..NONE
    }),
    entry("Hostinger Website Builder", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Hostinger"), bw(r"\.zyrosite\.com/", 2.0)],
        headers: &[
            ("x-powered-by", &[h(r"HostingerWebsiteBuilder")]),
        ],
    }),
    // Google's ESF server header is shared by many services; only the resources of
    // Google Sites itself are a feature
    entry("Google Sites", Signature {
        body: &[bw(r"gstatic\.com/atari/", 2.0)],
        ..NONE
    }),
    entry("Readymag", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Readymag"), bw(r"\.rmcdn\.net/", 2.0)],
        ..NONE
    }),
    // Cargo 2 serves files from cargocollective.com, Cargo 3 from cargo.site and
    // names itself in headers
    entry("Cargo", Signature {
        body: &[bw(r"\.cargocollective\.com/", 2.0), b(r"freight\.cargo\.site")],
        headers: &[
            ("x-powered-by", &[h(r"\bCargo\b")]),
        ],
    }),
    // Shopware 6 builds the theme into `/theme/<md5>/`, Shopware 5 keeps it in
    // `/themes/Frontend/`
    entry("Shopware", Signature {
        body: &[bw(r"/theme/[0-9a-f]{32}/(css|js)/", 2.0), b(r"window\.router\['frontend\."), bw(r"/themes/Frontend/", 2.0)],
        headers: &[
            ("sw-language-id", &[h(r".+")]),
        ],
    }),
    entry("nopCommerce", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+nopCommerce"), b(r"/Themes/[^/]+/Content/")],
        headers: &[
            ("set-cookie", &[h(r"\.Nop\.")]),
        ],
    }),
    entry("VTEX", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+vtex\.render-server"), bw(r"vteximg\.com\.br", 2.0), bw(r"vtexassets\.com", 2.0)],
        headers: &[
            ("x-vtex-cache-status", &[h(r".+")]),
        ],
    }),
    entry("Shoptet", Signature {
        body: &[bw(r"cdn\.myshoptet\.com", 2.0), b(r"\bshoptet\.config")],
        ..NONE
    }),
    entry("Shoper", Signature {
        body: &[bw(r"/environment/cache/images/", 2.0)],
        headers: &[
            ("x-powered-by", &[h(r"DCSaaS")]),
        ],
    }),
    entry("Cafe24", Signature {
        body: &[bw(r"echosting\.cafe24\.com", 2.0), b(r"/ind-script/"), b(r"\bEC_FRONT")],
        ..NONE
    }),
    // `/web/assets/` without a leading quote matches Mobirise's
    // `assets/web/assets/`
    entry("Odoo", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Odoo"), b(r#"="/web/assets/"#), b(r"\bdata-oe-model")],
        ..NONE
    }),
    entry("Sylius", Signature {
        body: &[bw(r#"id="sylius-"#, 2.0), b(r"/bundles/sylius")],
        ..NONE
    }),
    entry("OXID eShop", Signature {
        body: &[b(r#"/out/[^/"]+/src/(js|css|img)/"#), b(r"<!-- OXID eShop")],
        headers: &[
            ("set-cookie", &[h(r"\bsid_key=")]),
        ],
    }),
    entry("Gambio", Signature {
        body: &[bw(r"\bdata-gambio-(namespace|controller|widget)", 2.0), b(r"GXModules/")],
        ..NONE
    }),
    entry("JTL-Shop", Signature {
        body: &[bw(r"\bjtl_token", 2.0), b(r"templates/NOVA/")],
        headers: &[
            ("set-cookie", &[h(r"\bJTLSHOP=")]),
        ],
    }),
    entry("Big Cartel", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Big Cartel"), bw(r"assets\.bigcartel\.com", 2.0)],
        ..NONE
    }),
    // The platform's former name, 3dcart, remains in store markup
    entry("Shift4Shop", Signature {
        body: &[bw(r"<!--\s*START: 3dcart", 2.0), b(r"/assets/templates/common-html5/"), b(r"[?&]vcart=[0-9]")],
        ..NONE
    }),
    entry("Simple Machines Forum", Signature {
        body: &[bw(r"\bsmf_(scripturl|theme_url)", 2.0), b(r"/Themes/default/scripts/")],
        ..NONE
    }),
    entry("NodeBB", Signature {
        body: &[bw(r"/assets/nodebb\.min\.js", 2.0)],
        headers: &[
            ("x-powered-by", &[h(r"NodeBB")]),
        ],
    }),
    entry("Flarum", Signature {
        body: &[bw(r#"\bid="flarum-loading"#, 2.0)],
        headers: &[
            ("set-cookie", &[h(r"\bflarum_session=")]),
            ("x-powered-by", &[h(r"Flarum")]),
        ],
    }),
    entry("DokuWiki", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+DokuWiki"), b(r"/lib/exe/")],
        headers: &[
            ("set-cookie", &[h(r"\bDokuWiki=")]),
        ],
    }),
    // DNN runs on ASP.NET, which scores up to 7 (`__VIEWSTATE` and two headers):
    // DNN's own feature weighs the same and wins because it is listed higher
    entry("DNN", Signature {
        body: &[bw(r"/DesktopModules/", 7.0), bw(r"/Portals/[0-9_]+/", 2.0), bw(r"<meta[^>]+generator[^>]+DotNetNuke", 7.0)],
        headers: &[
            ("set-cookie", &[h(r"\bdnn_IsMobile=")]),
        ],
    }),
    entry("Grav", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+GravCMS")],
        headers: &[
            ("set-cookie", &[h(r"\bgrav-site-")]),
        ],
    }),
    // Statamic and BookStack run on Laravel and send its cookies, worth up to 4,
    // so their own feature weighs more; otherwise the answer would be Laravel
    entry("Statamic", Signature {
        headers: &[
            ("x-powered-by", &[hw(r"Statamic", 5.0)]),
        ],
        ..NONE
    }),
    entry("BookStack", Signature {
        headers: &[
            ("set-cookie", &[hw(r"\bbookstack_session=", 5.0)]),
        ],
        ..NONE
    }),
    entry("ProcessWire", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+ProcessWire"), b(r"/site/(templates|assets)/")],
        headers: &[
            ("x-powered-by", &[h(r"ProcessWire")]),
            ("set-cookie", &[h(r"\bwires=")]),
        ],
    }),
    // The `_resources/` path counts only from the root: Albireo CMS keeps it
    // inside `/templates/` (max-3000.com)
    entry("Silverstripe", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Silverstripe"), b(r#"="(https?://[^/"]+)?/_resources/"#)],
        ..NONE
    }),
    entry("Storyblok", Signature {
        body: &[b(r"a\.storyblok\.com")],
        ..NONE
    }),
    entry("DatoCMS", Signature {
        body: &[b(r"datocms-assets\.com")],
        ..NONE
    }),
    entry("LiveStreet CMS", Signature {
        body: &[bw(r"\bLIVESTREET_SECURITY_KEY", 2.0), b(r"/templates/skin/")],
        ..NONE
    }),
    entry("Bolt CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Bolt")],
        headers: &[
            ("x-powered-by", &[h(r"\bBolt\b")]),
        ],
    }),
    // Bitrix24 sites load the same `/bitrix/js/landing/` as on-premise 1C-Bitrix;
    // only the cloud header tells them apart
    entry("Bitrix24.Sites", Signature {
        headers: &[
            ("server", &[hw(r"Bitrix24\.Sites", 5.0)]),
            ("x-powered-cms", &[h(r"Bitrix24\.Sites")]),
        ],
        ..NONE
    }),
    // Carrd writes the page script itself; the "Made with Carrd" badge exists only
    // on free sites
    entry("Carrd", Signature {
        body: &[bw(r"var on = addEventListener,\s*off = removeEventListener", 3.0), b(r"\bicc-credits")],
        ..NONE
    }),
    entry("ClickFunnels", Signature {
        body: &[bw(r"statics\.myclickfunnels\.com", 2.0), bw(r#"id="cf-head-scripts""#, 2.0)],
        ..NONE
    }),
    entry("SITE123", Signature {
        body: &[bw(r"cdn-cms\.f-static\.com", 2.0), b(r"\bs123-cdn-network")],
        ..NONE
    }),
    entry("Systeme.io", Signature {
        body: &[bw(r"<!-- Created with https://systeme\.io", 3.0), b(r"d1yei2z3i6k35z\.cloudfront\.net")],
        ..NONE
    }),
    // Showit draws the pages, but the blog on its sites usually runs on WordPress,
    // which puts up to five of its features into the markup
    entry("Showit", Signature {
        body: &[bw(r"static\.showit\.co", 6.0)],
        ..NONE
    }),
    entry("Bubble", Signature {
        body: &[bw(r"\b_bubble_page_load_data", 2.0), b(r"\bbubble_session_uid")],
        ..NONE
    }),
    // The `/media/pages/` path itself is generic; Kirby gives itself away by its
    // fingerprint: a ten-character hash and a timestamp before the file name
    entry("Kirby", Signature {
        body: &[bw(r#"/media/(pages|site)/[^"'\s]*[0-9a-f]{10}-[0-9]{10}/"#, 3.0)],
        ..NONE
    }),
    entry("Pimcore", Signature {
        headers: &[
            ("x-powered-by", &[h(r"\bpimcore\b")]),
        ],
        ..NONE
    }),
    entry("Docusaurus", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Docusaurus")],
        ..NONE
    }),
    entry("MkDocs", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+mkdocs")],
        ..NONE
    }),
    entry("VitePress", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+VitePress")],
        ..NONE
    }),
    entry("GitBook", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+GitBook")],
        ..NONE
    }),
    entry("Movable Type", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Movable Type"), b(r"/mt-static/")],
        ..NONE
    }),
    // Teachable and Thinkific run on Rails, Salla and XpressEngine 3 on Laravel,
    // and the framework sends its features worth up to 4: the platform feature is
    // heavier
    entry("XpressEngine", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+XpressEngine", 5.0)],
        ..NONE
    }),
    entry("Sulu", Signature {
        headers: &[
            ("x-generator", &[h(r"\bSulu/")]),
        ],
        ..NONE
    }),
    entry("Substack", Signature {
        headers: &[
            ("x-served-by", &[h(r"\bSubstack\b")]),
        ],
        ..NONE
    }),
    entry("GetCourse", Signature {
        body: &[bw(r"\bfileServiceThumbnailHost", 2.0), b(r#"class="gc-user-(guest|logged)"#), b(r"fs\.getcourse\.ru/fileservice/")],
        ..NONE
    }),
    // Invision Community 4 marks up the page with ipsLayout_ classes; version 3
    // loads styles from public/style_css
    entry("Invision Community", Signature {
        body: &[bw(r"\bipsLayout_", 2.0), bw(r"public/style_css/css_[0-9]+/ipb_", 2.0)],
        ..NONE
    }),
    entry("Teachable", Signature {
        body: &[bw(r"teachablecdn\.com", 5.0)],
        ..NONE
    }),
    entry("Thinkific", Signature {
        body: &[bw(r"window\.Thinkific\.", 5.0), b(r"\bthinkific-cdn"), b(r"assets\.thinkific\.com")],
        ..NONE
    }),
    entry("Salla", Signature {
        body: &[bw(r"cdn\.salla\.(network|sa)", 5.0)],
        ..NONE
    }),
    entry("Tumblr", Signature {
        body: &[bw(r"pre_tumblelog\.js", 2.0), b(r"assets\.tumblr\.com/client/")],
        ..NONE
    }),
    entry("Gnuboard", Signature {
        body: &[bw(r"var g5_url", 2.0), bw(r"\bg4_path", 2.0)],
        ..NONE
    }),
    entry("X-Cart", Signature {
        body: &[bw(r#"type="text/x-cart-data""#, 2.0), b(r"<meta[^>]+generator[^>]+X-Cart")],
        ..NONE
    }),
    entry("Hexo", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Hexo")],
        ..NONE
    }),
    entry("Eleventy", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Eleventy")],
        ..NONE
    }),
    entry("VuePress", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+VuePress")],
        ..NONE
    }),
    entry("Mintlify", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Mintlify")],
        ..NONE
    }),
    entry("Pelican", Signature {
        body: &[b(r#"href="https?://getpelican\.com"#)],
        ..NONE
    }),
    entry("Sphinx", Signature {
        body: &[bw(r"_static/documentation_options\.js", 2.0)],
        ..NONE
    }),
    entry("Nicepage", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Nicepage"), bw(r"/nicepage\.css", 2.0)],
        ..NONE
    }),
    entry("Softr", Signature {
        body: &[bw(r"assets\.softr-files\.com", 2.0)],
        ..NONE
    }),
    entry("SAP Commerce Cloud", Signature {
        body: &[bw(r"/_ui/responsive/", 2.0), b(r"\bACC\.config")],
        ..NONE
    }),
    entry("BASE", Signature {
        body: &[bw(r"cf-baseassets\.thebase\.in", 2.0), b(r"\bbaseec-img-mng")],
        ..NONE
    }),
    entry("Builder.io", Signature {
        body: &[b(r"cdn\.builder\.io/api/v1/image/")],
        ..NONE
    }),
    entry("Contentstack", Signature {
        body: &[b(r"images\.contentstack\.(io|com)/v3/assets/")],
        ..NONE
    }),
    // A link to business.yandex.ru is not a feature: agencies that set up ads put
    // it there too. Recognised by the "Site created in Yandex Business" badge and
    // images from the business directory
    entry("Yandex Business", Signature {
        body: &[bw(r#"class="Branding_branding__"#, 2.0), b(r"avatars\.mds\.yandex\.net/get-(sprav|altay)")],
        ..NONE
    }),
    entry("Discuz!", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Discuz!")],
        headers: &[
            ("set-cookie", &[h(r"\b[a-z0-9]{4}_[0-9]{4}_saltkey=")]),
        ],
    }),
    entry("LiveJournal", Signature {
        body: &[bw(r"l-stat\.livejournal\.net", 2.0)],
        ..NONE
    }),
    entry("SkeekS CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+SkeekS CMS")],
        headers: &[
            ("x-powered-cms", &[h(r"SkeekS CMS")]),
        ],
    }),
    // The server of some MaxSite sites sets a `wordpress_no_cache` cookie, and
    // WordPress scores 2
    entry("MaxSite CMS", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+MaxSite CMS", 2.0), bw(r"/application/maxsite/", 2.0)],
        ..NONE
    }),
    entry("Danneo CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Danneo")],
        headers: &[
            ("x-powered-by", &[h(r"\bDanneo\b")]),
        ],
    }),
    entry("Twilight CMS", Signature {
        headers: &[
            ("x-powered-cms", &[h(r"Twilight CMS")]),
        ],
        ..NONE
    }),
    entry("S-COREpion CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+S-COREpion")],
        headers: &[
            ("x-powered-by", &[h(r"S-COREpion")]),
            ("set-cookie", &[h(r"\bSCOREPION=")]),
        ],
    }),
    entry("Aegea", Signature {
        body: &[b(r"\be2-(note|text)\b")],
        headers: &[
            ("x-powered-by", &[h(r"\bAegea\b")]),
        ],
    }),
    entry("Vigbo", Signature {
        body: &[b(r"vigbo\.com/cms/"), b(r"\bvigbo-cms/")],
        headers: &[
            ("set-cookie", &[h(r"\b_gphw_")]),
        ],
    }),
    entry("Eleanor CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Eleanor CMS")],
        ..NONE
    }),
    entry("CMS Made Simple", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+CMS Made Simple")],
        headers: &[
            ("set-cookie", &[h(r"\bCMSSESSID")]),
        ],
    }),
    entry("e107", Signature {
        body: &[bw(r"\be107_(themes|images|plugins|files)/", 2.0)],
        headers: &[
            ("x-powered-by", &[h(r"\be107\b")]),
            ("set-cookie", &[h(r"\be107_csrf="), h(r"\bSESSE107COOKIE")]),
        ],
    }),
    entry("XOOPS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+XOOPS")],
        ..NONE
    }),
    // Publii builds a static site that gets uploaded to any hosting: the server
    // header there comes from IIS and ASP.NET, i.e. 2 points to another name
    entry("Publii", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Publii", 3.0)],
        ..NONE
    }),
    entry("Bludit", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Bludit")],
        headers: &[
            ("x-powered-by", &[h(r"\bBludit\b")]),
        ],
    }),
    entry("Microweber", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Microweber")],
        headers: &[
            ("set-cookie", &[h(r"\bmw_com_session")]),
        ],
    }),
    // Payload sends its own header next to Next.js and pairs with it, like
    // Storyblok and DatoCMS: the site is built by the frontend, and the content
    // lives in Payload
    entry("Payload CMS", Signature {
        headers: &[
            ("x-powered-by", &[h(r"\bPayload\b")]),
        ],
        ..NONE
    }),
    // Django CMS runs on Django, which sends its own features: the plugin name is
    // heavier
    entry("Django CMS", Signature {
        body: &[bw(r"\bdjangocms[_-]", 5.0)],
        ..NONE
    }),
    entry("Neos CMS", Signature {
        body: &[b(r"_Resources/Static/Packages/"), b(r"/_Resources/Persistent/"), b(r"\bneos-contentcollection")],
        ..NONE
    }),
    entry("eZ Publish", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+eZ Publish")],
        headers: &[
            ("x-powered-by", &[h(r"eZ Publish")]),
            ("set-cookie", &[h(r"\beZSESSID=")]),
        ],
    }),
    entry("EC-CUBE", Signature {
        headers: &[
            ("set-cookie", &[h(r"\beccube="), h(r"\bECSESSID=")]),
        ],
        ..NONE
    }),
    entry("Backdrop CMS", Signature {
        body: &[b(r#"<meta[^>]+content="Backdrop CMS"#)],
        headers: &[
            ("x-generator", &[h(r"Backdrop CMS")]),
        ],
    }),
    entry("Serendipity", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Serendipity")],
        headers: &[
            ("set-cookie", &[h(r"\bs9y_[0-9a-f]")]),
        ],
    }),
    entry("Typecho", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Typecho")],
        ..NONE
    }),
    entry("PHPFusion", Signature {
        body: &[b(r"\binfusions/")],
        headers: &[
            ("set-cookie", &[h(r"\bfusion[a-zA-Z0-9]*_(visited|lastvisit)=")]),
        ],
    }),
    entry("SPIP", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+SPIP"), b(r"local/cache-(gd|vigne)"), b(r"spip\.php\?")],
        headers: &[
            ("set-cookie", &[h(r"\bspip_lang=")]),
        ],
    }),
    entry("DESTOON", Signature {
        headers: &[
            ("x-powered-by", &[h(r"\bDESTOON\b")]),
        ],
        ..NONE
    }),
    entry("DedeCMS", Signature {
        body: &[b(r"\bDedeAjax")],
        ..NONE
    }),
    entry("XWiki", Signature {
        body: &[b(r#"class="xwiki-"#)],
        ..NONE
    }),
    entry("Foswiki", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Foswiki"), b(r#"class="[^"]*foswiki"#)],
        headers: &[
            ("set-cookie", &[h(r"\bFOSWIKI")]),
        ],
    }),
    entry("MoinMoin", Signature {
        body: &[b(r"/moin_static")],
        ..NONE
    }),
    entry("PunBB", Signature {
        body: &[b(r"\bPUNBB\.env"), b(r"punbb\.common"), b(r#"\bid="brd-main""#)],
        ..NONE
    }),
    // FluxBB forked from PunBB 1.2 and carries its markup: a forum on that ancient
    // PunBB branch answers FluxBB. PunBB recognises its own version by the three
    // features above
    entry("FluxBB", Signature {
        body: &[b(r#"\bid="brdmain""#)],
        ..NONE
    }),
    entry("Vanilla", Signature {
        body: &[b(r"\bvanilla/js/")],
        headers: &[
            ("set-cookie", &[h(r"\bVanilla=")]),
        ],
    }),
    entry("Hatena Blog", Signature {
        body: &[b(r"cdn\.blog\.st-hatena\.com"), b(r"hatenablog-parts\.com")],
        ..NONE
    }),
    entry("Tistory", Signature {
        body: &[b(r"tistory[0-9]?\.daumcdn\.net"), b(r"daumcdn\.net/tistory_")],
        ..NONE
    }),
    entry("Pixnet", Signature {
        body: &[b(r"\b(static|pimg)\.1px\.tw"), b(r"pic\.pimg\.tw")],
        ..NONE
    }),
    entry("ColorMeShop", Signature {
        body: &[b(r"\bimg\.shop-pro\.jp")],
        headers: &[
            ("set-cookie", &[h(r"\bcolorme_")]),
        ],
    }),
    entry("Upgates", Signature {
        body: &[b(r"upgates\.com/_cache/")],
        headers: &[
            ("set-cookie", &[h(r"\bUPGATES_")]),
        ],
    }),
    entry("GetSimple CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+GetSimple")],
        ..NONE
    }),
    entry("CMSimple", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+CMSimple")],
        ..NONE
    }),
    entry("ImpressPages", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+ImpressPages")],
        ..NONE
    }),
    entry("CubeCart", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+cubecart")],
        headers: &[
            ("set-cookie", &[h(r"\bCCS_[0-9A-F]{8,}"), h(r"\bcc_currency=")]),
        ],
    }),
    entry("AbanteCart", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+AbanteCart")],
        headers: &[
            ("set-cookie", &[h(r"\bAC_SF_")]),
        ],
    }),
    entry("Rhymix", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Rhymix")],
        headers: &[
            ("set-cookie", &[h(r"\brx_login_status")]),
        ],
    }),
    // The cookie name of a Spree store depends on the application:
    // `_spree_starter_session`, `_<store>_spree_session`
    entry("Spree", Signature {
        headers: &[
            ("set-cookie", &[h(r"spree[a-z0-9_]*_session")]),
        ],
        ..NONE
    }),
    entry("OpenCms", Signature {
        body: &[b(r"\bopencms\."), b(r"/export/system/")],
        ..NONE
    }),
    // eZ Platform is the former name of Ibexa DXP (version 2); sites on it answer
    // with the same header
    entry("Ibexa DXP", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Ibexa")],
        headers: &[
            ("x-powered-by", &[h(r"\bIbexa\b"), h(r"eZ Platform")]),
        ],
    }),
    entry("ikas", Signature {
        headers: &[
            ("x-powered-by", &[h(r"\bikas\b")]),
        ],
        ..NONE
    }),
    entry("Brightspot", Signature {
        headers: &[
            ("x-powered-by", &[h(r"\bBrightspot\b")]),
        ],
        ..NONE
    }),
    entry("RedCart", Signature {
        headers: &[
            ("set-cookie", &[h(r"\brc2c-")]),
        ],
        ..NONE
    }),
    entry("ShopGold", Signature {
        headers: &[
            ("set-cookie", &[h(r"\beGold=")]),
        ],
        ..NONE
    }),
    entry("Sazito", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Sazito")],
        headers: &[
            ("set-cookie", &[h(r"\bsazito_session")]),
        ],
    }),
    entry("Dukaan", Signature {
        body: &[b(r"mydukaan\.io")],
        ..NONE
    }),
    // Haravan copies Shopify down to the `x-shopid` header, and its stores came
    // out as Shopify: the platform's CDN domain weighs more
    entry("Haravan", Signature {
        body: &[bw(r"\bhstatic\.net", 3.0)],
        ..NONE
    }),
    entry("Bsale", Signature {
        body: &[b(r"\bbsale\.cl")],
        headers: &[
            ("set-cookie", &[h(r"_bsalemarket_session")]),
        ],
    }),
    entry("Cotonti", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Cotonti")],
        ..NONE
    }),
    entry("CONTENIDO", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+CONTENIDO")],
        headers: &[
            ("set-cookie", &[h(r"\b1frontend=")]),
        ],
    }),
    entry("Fork CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Fork CMS")],
        ..NONE
    }),
    entry("Batflat", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Batflat")],
        ..NONE
    }),
    // An offline site builder uploads ready-made files and keeps no server of its
    // own: recognised by its generator and the directory where it puts shared
    // resources
    entry("Adobe Muse", Signature {
        body: &[b(r"\bmuseutils\.js"), b(r"\bmuseconfig\.js"), b(r"window\.Muse\.assets"), b(r"jquery\.musepolyfill")],
        ..NONE
    }),
    entry("WebSite X5", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+WebSite X5"), b(r"\bx5engine")],
        ..NONE
    }),
    // The builder's ready-made files sit on any server, and at just-normlicht.com
    // that is ASP.NET with `__VIEWSTATE`: the generator weighs more, so that the
    // answer is the site's engine rather than the platform under it
    entry("Zeta Producer", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Zeta Producer", 7.0), b(r"\bzpnodefaults"), b(r"\bzpColumnItem"), b(r"\bzpwButton")],
        ..NONE
    }),
    entry("EverWeb", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+EverWeb"), b(r"\bew_css/"), b(r"\bew_js/")],
        ..NONE
    }),
    entry("RapidWeaver", Signature {
        body: &[b(r"\brw_common/")],
        ..NONE
    }),
    // SitePad is built on a WordPress fork and serves the same files
    // (`wp-embed.min.js` from its own `/site-inc/js/`, `page-template-default`
    // classes), so its generator weighs more; otherwise the answer becomes
    // WordPress
    entry("SitePad", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+SitePad", 3.0)],
        ..NONE
    }),
    // The platform's name in `og:site_name` and `twitter:title` is not a feature:
    // Textpattern was declared this way by its own forum and its Jekyll
    // documentation, and WikkaWiki by a WordPress blog under wikkawiki.org
    entry("Textpattern CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Textpattern")],
        ..NONE
    }),
    entry("Tiki Wiki CMS Groupware", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Tiki Wiki"), b(r"\btiki-index\.php"), b(r"\btiki-[a-z_]+_rss\.php")],
        ..NONE
    }),
    entry("TiddlyWiki", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+TiddlyWiki"), b(r"\btc-btn-invisible"), b(r"\btc-image-button")],
        ..NONE
    }),
    entry("WikkaWiki", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+WikkaWiki")],
        ..NONE
    }),
    entry("YesWiki", Signature {
        body: &[b(r"\byw-topnav"), b(r"\byw-main"), b(r"\byw-footer"), b(r"/tools/bazar/")],
        ..NONE
    }),
    // `twikiLink` is not a feature, even though the class belongs to TWiki: in
    // TiddlyWiki markup it matches at the end of an escaped `\t\twikiLinks`,
    // because there is a word boundary between a backslash and a letter too
    entry("TWiki", Signature {
        body: &[b(r"\btwikiTable"), b(r"/twiki/pub/"), b(r"/twiki/bin/")],
        ..NONE
    }),
    entry("Apache JSPWiki", Signature {
        body: &[b(r"\bWiki\.jsp")],
        ..NONE
    }),
    entry("Atlassian Confluence", Signature {
        body: &[b(r"\bconfluence-base-url"), b(r"\bconfluence-context-path"), b(r"\bconfluence-embedded-file-wrapper"), b(r"confluence\.fe\.page-load")],
        ..NONE
    }),
    entry("ikiwiki", Signature {
        body: &[b(r"\bikiwiki\.cgi")],
        ..NONE
    }),
    entry("Omeka", Signature {
        body: &[b(r"/application/views/scripts/"), b(r"\bOmeka\.(?:skipNav|showAdvancedForm)")],
        ..NONE
    }),
    entry("Elgg", Signature {
        body: &[b(r"\belgg-body"), b(r"\belgg-menu-"), b(r"\belgg-module"), b(r"\b__elgg_token")],
        headers: &[
            ("set-cookie", &[h(r"\bElgg=")]),
        ],
    }),
    entry("HumHub", Signature {
        body: &[b(r"\bhumhub-app\.(?:js|css)")],
        ..NONE
    }),
    // Federated platforms run on frameworks that send their own cookies (Rails for
    // Mastodon, Laravel for Pixelfed), so their own feature weighs more: the
    // answer must be the engine, not the framework under it. A bare
    // `id="mastodon"` is not a feature: that id is used for a link icon to
    // Mastodon in other people's markup (`<symbol id="mastodon">` on
    // thenew.institute, a Kirby site). Mastodon's own app hangs its state on the
    // same node in `data-props` and serves its themes as its own pack
    entry("Mastodon", Signature {
        body: &[bw(r#"id="mastodon"[^>]*data-props"#, 5.0), bw(r#"data-props="[^"]*"\s+id="mastodon""#, 5.0), bw(r"/packs/[a-z]+/mastodon", 2.0)],
        ..NONE
    }),
    entry("Misskey", Signature {
        body: &[b(r"<meta[^>]+application-name[^>]+Misskey")],
        ..NONE
    }),
    entry("Pleroma", Signature {
        body: &[b(r"/api/pleroma/")],
        ..NONE
    }),
    entry("Pixelfed", Signature {
        body: &[bw(r"\bpixelfed-icon-color", 5.0)],
        ..NONE
    }),
    entry("PeerTube", Signature {
        body: &[b(r#"og:platform"\s+content="PeerTube"#), b(r#"content="PeerTube"\s+property="og:platform"#)],
        ..NONE
    }),
    entry("Mattermost", Signature {
        body: &[b(r"<meta[^>]+application-name[^>]+Mattermost")],
        ..NONE
    }),
    entry("MyBB", Signature {
        body: &[b(r"jscripts/general\.js"), b(r"\bmy_post_key")],
        ..NONE
    }),
    // `index.php?t=` is not a feature, even though it is FUDforum's URL scheme:
    // the same string showed up in a visit counter on swarajyamag.com (Quintype)
    // and on thequint.com. The forum is recognised by its cookie
    entry("FUDforum", Signature {
        headers: &[
            ("set-cookie", &[h(r"\bfud_session")]),
        ],
        ..NONE
    }),
    entry("XMB", Signature {
        body: &[b(r"\bctrtablerow"), b(r"\brghttablerow")],
        headers: &[
            ("set-cookie", &[h(r"\bxmblv")]),
        ],
    }),
    entry("YaBB", Signature {
        body: &[b(r"\bYaBB\.pl")],
        ..NONE
    }),
    entry("Philomena", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Philomena"), b(r#"content="Philomena"#)],
        ..NONE
    }),
    entry("Gridsome", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Gridsome")],
        ..NONE
    }),
    entry("Quarto", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+quarto-")],
        ..NONE
    }),
    entry("Nextra", Signature {
        body: &[b(r"\bnextra-")],
        ..NONE
    }),
    entry("Retype", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Retype")],
        ..NONE
    }),
    entry("Rspress", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Rspress")],
        ..NONE
    }),
    // Cecil and Lume write `<meta content="… " name=generator>`: the attribute
    // name comes after the value and without quotes, so the feature matches the
    // value
    entry("Cecil", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Cecil"), b(r#"content="Cecil [0-9]"#)],
        ..NONE
    }),
    // Octopress is a layer over Jekyll and declares both in one generator line:
    // its own feature weighs more; otherwise the answer becomes Jekyll
    entry("Octopress", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Octopress", 2.0), b(r"\boctopress\.js")],
        ..NONE
    }),
    entry("Lume", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+\bLume\b"), b(r#"content="Lume [0-9a-f]"#)],
        ..NONE
    }),
    entry("Saber", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Saber")],
        ..NONE
    }),
    entry("Scully", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Scully")],
        ..NONE
    }),
    entry("Bridgetown", Signature {
        body: &[b(r"/_bridgetown/")],
        ..NONE
    }),
    // The blogging platform is recognised by its CDN host: it stays even after the
    // blog moves to the owner's domain
    entry("TypePad", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+typepad")],
        ..NONE
    }),
    entry("JUGEM", Signature {
        body: &[b(r"\bjugemTracker"), b(r"\bjugemkey\.jp")],
        ..NONE
    }),
    entry("Postach.io", Signature {
        body: &[b(r"cdn-static\.postach\.io")],
        ..NONE
    }),
    // Svbtle is a Rails app, and `authenticity_token` weighs 2: its own feature
    // weighs more; otherwise the answer becomes the framework
    entry("Svbtle", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Svbtle", 3.0), bw(r"lightning\.svbtle\.com", 3.0)],
        ..NONE
    }),
    entry("Superblog", Signature {
        body: &[b(r"\bsuperblogcdn\.com")],
        ..NONE
    }),
    entry("Notion", Signature {
        body: &[b(r"\bnotion-html"), b(r"\bnotion-version")],
        ..NONE
    }),
    entry("Super", Signature {
        body: &[b(r#"<meta[^>]+generator[^>]+"Super""#)],
        ..NONE
    }),
    entry("Canva", Signature {
        body: &[b(r"\b__canva_website_bootstrap__"), b(r"\bcanva_scriptExecutor")],
        ..NONE
    }),
    entry("Lovable", Signature {
        body: &[b(r"<meta[^>]+author[^>]+Lovable")],
        ..NONE
    }),
    entry("Umso", Signature {
        body: &[b(r"\bumsousercontent\.com")],
        ..NONE
    }),
    entry("Instapage", Signature {
        body: &[b(r"\bfastcdn\.co")],
        ..NONE
    }),
    entry("Plasmic", Signature {
        body: &[b(r"\bplasmic-"), b(r"\b__wab_")],
        ..NONE
    }),
    entry("Voog", Signature {
        body: &[b(r"static\.voog\.com")],
        ..NONE
    }),
    entry("Pixpa", Signature {
        body: &[b(r"\bpixpa-menu"), b(r"\bpixpa-marqueesize")],
        ..NONE
    }),
    entry("WebWave", Signature {
        body: &[b(r"\bwebwave\.isDefAndNotNull")],
        ..NONE
    }),
    // A bare `ycode` cannot be a feature: the fragment sits inside ordinary
    // `currencyCode` and `countryCode`, and sites on Mono.net were named Ycode
    // because of it
    entry("Ycode", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Ycode")],
        ..NONE
    }),
    entry("GreatPages", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+GreatPages")],
        ..NONE
    }),
    entry("STUDIO", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Studio\.Design")],
        ..NONE
    }),
    entry("Mono.net", Signature {
        body: &[b(r"\bcdnmns\.com")],
        ..NONE
    }),
    entry("LadiPage", Signature {
        body: &[b(r"\bladicdn\.com")],
        ..NONE
    }),
    entry("Pagecloud", Signature {
        body: &[b(r"app-assets\.pagecloud\.com")],
        ..NONE
    }),
    entry("KlickPages", Signature {
        body: &[b(r"static-public\.kpages\.com\.br")],
        ..NONE
    }),
    // Oopy and Peraichi serve Notion content but from their own CDN hosts: their
    // pages have no `notion-html` and `notion-version` markers, so there is no
    // conflict with Notion
    entry("Oopy", Signature {
        body: &[bw(r"oopy\.lazyrockets\.com", 2.0)],
        ..NONE
    }),
    // The platform's name in markup means nothing: at notosiki.co.jp it is a
    // banner advertising it (`top_main_peraichi.png`). The feature is the host of
    // its pages, and it weighs more than Django, which the platform itself is
    // written in
    entry("Peraichi", Signature {
        body: &[bw(r"\bperaichiapp\.com", 3.0)],
        ..NONE
    }),
    entry("CKAN", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+\bckan\b")],
        ..NONE
    }),
    // Arc XP builds the page with its Fusion engine, and that name appears in the
    // markup of every block
    entry("Arc XP", Signature {
        body: &[b(r"\bfusion-app"), b(r"/pf/dist/")],
        ..NONE
    }),
    entry("Quintype", Signature {
        body: &[b(r"\bquintype\.io"), b(r"\bqlitics\.com"), b(r"\bquintype-ace/")],
        ..NONE
    }),
    entry("RebelMouse", Signature {
        body: &[b(r"assets\.rebelmouse\.io")],
        ..NONE
    }),
    entry("WHMCS", Signature {
        body: &[b(r"\bwhmcsBaseUrl")],
        ..NONE
    }),
    entry("WoltLab Suite", Signature {
        body: &[b(r"\bWCF\.[A-Z]")],
        headers: &[
            ("set-cookie", &[h(r"\bw_user_session")]),
        ],
    }),
    entry("NamelessMC", Signature {
        body: &[b(r"\bnameless-deps-dist"), b(r"\bnamelessmc_")],
        ..NONE
    }),
    entry("Jahia", Signature {
        body: &[b(r"/modules/javascript-modules-engine/")],
        ..NONE
    }),
    entry("Finalsite", Signature {
        body: &[b(r"\bfsElement"), b(r"\bfinalsite\.net")],
        ..NONE
    }),
    entry("E-monsite", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+e-monsite")],
        ..NONE
    }),
    entry("JouwWeb", Signature {
        body: &[b(r"assets\.jwwb\.nl")],
        ..NONE
    }),
    entry("Flazio", Signature {
        body: &[b(r"\bflazio\.org")],
        ..NONE
    }),
    entry("MotoCMS", Signature {
        body: &[b(r"/mt-includes/"), b(r"/mt-content/")],
        ..NONE
    }),
    entry("Indexhibit", Signature {
        body: &[b(r"\bndxz-studio"), b(r"<meta[^>]+generator[^>]+Indexhibit")],
        ..NONE
    }),
    entry("ReadMe", Signature {
        body: &[b(r"cdn\.readme\.io")],
        ..NONE
    }),
    entry("Indico", Signature {
        body: &[b(r"\bIndicoCSPNonce"), b(r"\bindico\.modules\.")],
        ..NONE
    }),
    entry("NukeViet", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+NukeViet")],
        ..NONE
    }),
    // PyroCMS runs on Laravel, which sends its cookies worth up to 4: the platform
    // feature weighs more; otherwise the answer would be the framework
    entry("PyroCMS", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+PyroCMS", 5.0)],
        ..NONE
    }),
    entry("Subrion", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Subrion")],
        ..NONE
    }),
    // The name Sitefinity in text means nothing, it is on every vendor page about
    // it: the feature is its own namespaces in the markup
    entry("Sitefinity", Signature {
        body: &[b(r"\bSitefinity\.(?:Pages|Frontend|Services)"), b(r"\bSitefinitySiteMap")],
        ..NONE
    }),
    entry("SiteVision", Signature {
        body: &[b(r"\bSiteVision\.css"), b(r"\bsitevision-")],
        ..NONE
    }),
    entry("Botble CMS", Signature {
        body: &[b(r"themes/botble/")],
        ..NONE
    }),
    entry("UNA", Signature {
        body: &[b(r"/inc/js/classes/BxDol")],
        ..NONE
    }),
    entry("Weblication", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Weblication")],
        ..NONE
    }),
    // Dynamicweb and Elcom run on ASP.NET, and `__VIEWSTATE` with its two headers
    // weighs up to 7: the platform feature weighs no less, a tie goes to the entry
    // listed higher, and ASP.NET sits at the end of the table
    entry("Dynamicweb", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Dynamicweb", 7.0)],
        ..NONE
    }),
    entry("Thelia", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Thelia")],
        ..NONE
    }),
    entry("webEdition", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+webEdition")],
        ..NONE
    }),
    entry("Cloudrexx", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+cloudrexx")],
        ..NONE
    }),
    entry("Elcom", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+elcomCMS", 7.0)],
        ..NONE
    }),
    entry("Plate", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+\bPlate\b")],
        ..NONE
    }),
    entry("InterRed", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+interred")],
        ..NONE
    }),
    entry("Procurios", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Procurios")],
        ..NONE
    }),
    // A bare `pobo` cannot be a feature: Czech "pobočky" in Latin letters gives
    // `pobocky/` in a URL, and an unrelated store got the platform because of it
    entry("Pobo", Signature {
        body: &[b(r"\bpobo\.space")],
        ..NONE
    }),
    entry("Labrador CMS", Signature {
        body: &[b(r"/_labrador/")],
        ..NONE
    }),
    entry("Chayns", Signature {
        body: &[b(r"chayns-res\.tobit\.com")],
        ..NONE
    }),
    entry("MaxiCMS", Signature {
        body: &[b(r"\bmaxicms\.nl")],
        ..NONE
    }),
    entry("a-blog cms", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+a-blog cms")],
        ..NONE
    }),
    entry("OpenNemas", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+OpenNemas")],
        ..NONE
    }),
    entry("imperia CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+IMPERIA"), b(r"\bImperia-Live-Info"), b(r"imperia-website/")],
        ..NONE
    }),
    entry("K-Sup", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+K-Sup")],
        ..NONE
    }),
    entry("Jalios", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Jalios")],
        ..NONE
    }),
    // Ametys writes its generator backwards, like Cecil and Lume: the value comes
    // before the attribute name
    entry("Ametys", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Ametys"), b(r#"content="Ametys v"#)],
        ..NONE
    }),
    entry("Melis Platform", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Melis Platform")],
        ..NONE
    }),
    entry("Chameleon system", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Chameleon")],
        ..NONE
    }),
    entry("Hocalwire", Signature {
        body: &[b(r"\bhocalwire-")],
        ..NONE
    }),
    // Kooboo and C1 CMS run on ASP.NET (up to 7), iEXExchanger on Laravel (up to
    // 4): the platform feature weighs no less than its base
    entry("Kooboo CMS", Signature {
        body: &[bw(r"/Kooboo-(?:WebResource|Resource|Submit)/", 7.0)],
        ..NONE
    }),
    entry("C1 CMS", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+C1 CMS", 7.0)],
        ..NONE
    }),
    entry("iEXExchanger", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+iEXExchanger", 5.0), bw(r"\biexInitialGuideData", 5.0)],
        ..NONE
    }),
    entry("Pagekit", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Pagekit"), b(r"/packages/pagekit/")],
        ..NONE
    }),
    entry("ERPNext", Signature {
        body: &[b(r"/assets/erpnext/")],
        ..NONE
    }),
    entry("Ensi", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Ensi Platform")],
        ..NONE
    }),
    entry("Oracle Application Express", Signature {
        body: &[b(r"\bwwv_flow\."), b(r"\bapex_img_dir\b")],
        headers: &[
            ("set-cookie", &[h(r"\bORA_WWV_")]),
        ],
    }),
    entry("Powergap", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+POWERGAP")],
        ..NONE
    }),
    // Omni CMS puts resources under `/_resources/`, like Silverstripe, and
    // publishes to any server, IIS included: the editor login link with the
    // customer's skin must outweigh both
    entry("Omni CMS", Signature {
        body: &[bw(r"omniupdate\.com/(?:11/\?skin=|files/content\?)", 7.0)],
        ..NONE
    }),
    entry("DM Polopoly", Signature {
        body: &[b(r"\bpolopoly_fs(?:/|%2F)")],
        ..NONE
    }),
    // The CM4all builder is resold by hosting providers under their own brands
    // (Strato in Germany, planeetta.net in Finland), so the entry is named after
    // the engine, not a brand
    entry("CM4all", Signature {
        body: &[b(r"/\.cm4all/"), b(r"\bcm4all\.widgets")],
        ..NONE
    }),
    entry("HCL Commerce", Signature {
        body: &[b(r"/wcsstore/")],
        ..NONE
    }),
    // Shoplazza signs its response with an `x-powered-by: ASP.NET` header it does
    // not have, and Shopline and YouCan run on Laravel: the platform feature must
    // outweigh both
    entry("Shoplazza", Signature {
        body: &[bw(r"\bshoplazza://", 5.0), bw(r"\bshoplazza-product-snippet", 5.0)],
        ..NONE
    }),
    entry("Shopline", Signature {
        body: &[bw(r"\bshoplineimg\.com", 5.0)],
        ..NONE
    }),
    entry("YouCan", Signature {
        body: &[bw(r"\byoucan\.shop/(?:stores|store-front)/", 5.0)],
        ..NONE
    }),
    // IdoSell's `/gfx/<language>/` path is shared with other sites (larena.it on
    // Polopoly); the features are its classes and content directory
    entry("IdoSell Shop", Signature {
        body: &[b(r"\biai-(?:recaptcha|shop)"), b(r"\biai_cookie"), b(r"/data/include/cms/")],
        ..NONE
    }),
    entry("ePages", Signature {
        body: &[b(r#"/epages/[^/"'\s]+\.sf\b"#), b(r"\bepages\.(?:text|image|html|structure|base)\b")],
        ..NONE
    }),
    entry("Ueeshop", Signature {
        body: &[b(r"\bly200-cdn\.com"), b(r"\bueeshop_config\b")],
        ..NONE
    }),
    // A blogger.com link in share buttons and the plugin path `assets/components/`
    // give Blogger and MODX one point each on ShopBase and xt:Commerce stores: the
    // platform feature weighs 2; otherwise a tie goes to the entry listed higher
    entry("ShopBase", Signature {
        body: &[bw(r"img\.shopbase\.com/", 2.0)],
        ..NONE
    }),
    entry("Gomag", Signature {
        body: &[b(r"\bgomagcdn\.ro")],
        ..NONE
    }),
    entry("Unas", Signature {
        body: &[b(r"\.unas\.hu/element/")],
        ..NONE
    }),
    entry("Sky-Shop", Signature {
        body: &[b(r"\bSkyShopModule"), b(r"\bskyshop-container")],
        ..NONE
    }),
    entry("Intershop", Signature {
        body: &[b(r"/INTERSHOP/(?:static|web)/WFS/"), b(r"\bINTERSHOP\.enfinity")],
        ..NONE
    }),
    entry("xtCommerce", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+xt:Commerce", 2.0), b(r"/xtFramework/")],
        ..NONE
    }),
    entry("Bizweb", Signature {
        body: &[b(r"\bbizweb\.dktcdn\.net")],
        ..NONE
    }),
    entry("Miva", Signature {
        body: &[b(r"\bmerchant\.mvc\?"), b(r"\bmvga_tracker"), b(r"\bmivaJS\b")],
        ..NONE
    }),
    entry("Adobe Portfolio", Signature {
        body: &[b(r"\bcdn\.myportfolio\.com/"), b(r"\bpro2-bar-s3-cdn-cf\d*\.myportfolio\.com/")],
        ..NONE
    }),
    entry("Bloomreach", Signature {
        body: &[b(r"/binaries/(?:[a-z]+/)?content/gallery/"), b(r"\bhippo(?:gallery|std):"), b(r"\bhippoecm\.hst\b")],
        ..NONE
    }),
    entry("Stackbit", Signature {
        body: &[b(r"\bdata-sb-(?:object-id|field-path)\b"), b(r"\bstackbit_(?:model_type|page_meta)\b")],
        ..NONE
    }),
    entry("Kontent.ai", Signature {
        body: &[b(r"\bkc-usercontent\.com")],
        ..NONE
    }),
    entry("Unicorn Platform", Signature {
        body: &[b(r"\bunicorn-images\.b-cdn\.net"), b(r"unicornplatform\.com/static/")],
        ..NONE
    }),
    entry("Turbify", Signature {
        body: &[b(r"\bturbifycdn\.com/(?:ty|aah)/")],
        ..NONE
    }),
    entry("PubLive", Signature {
        body: &[b(r"\bpublive\.(?:online|com)/fit-in/"), b(r"\bpublive-(?:slot|logo|dynamic)")],
        ..NONE
    }),
    // A template exported from Webflow takes `data-wf-page` and `data-wf-site`
    // with it, and on Apostrophe (fanatee.com behind Express) Webflow scored as
    // much: Apostrophe's area markup weighs more
    entry("ApostropheCMS", Signature {
        body: &[bw(r"\bapos-(?:area|frontend|refreshable)\b", 2.0), bw(r"\bapostrophe-widgets\b", 2.0)],
        ..NONE
    }),
    // Tridion's content id `tcm:<publication>-<item>-<type>` gets into the markup
    // from a service comment on the page
    entry("SDL Tridion", Signature {
        body: &[b(r"\btcm:\d+-\d+-\d+\b")],
        ..NONE
    }),
    entry("Spring for creators", Signature {
        body: &[b(r"\bteespring\.com/v3/image/"), b(r"\bteespringId\b")],
        ..NONE
    }),
    entry("ButterCMS", Signature {
        body: &[b(r"\bcdn\.buttercms\.com/")],
        ..NONE
    }),
    // CrownPeak publishes to IIS, like Omni CMS, and ASP.NET scores up to 7 there
    entry("CrownPeak", Signature {
        body: &[bw(r"\bcrownpeak\.searchg2", 7.0)],
        ..NONE
    }),
    // Wagtail and Zid run on Django (up to 4), Shoprenter is an OpenCart fork with
    // its paths, SIDEARM publishes on ASP.NET, novomind iSHOP sends Laravel
    // cookies: the platform feature must outweigh its base. For Wagtail it is the
    // name of an image rendition: the filter (`fill-300x200`, `format-webp`) and
    // the key `2e16d0ba`
    entry("Wagtail", Signature {
        body: &[bw(r"\.(?:fill|width|height|max|min|scale)-\d+(?:x\d+)?(?:-c\d+)?\.format-(?:webp|avif|jpeg|png)\b", 5.0), bw(r"\.2e16d0ba\.(?:fill|width|height|max|min|scale|original)", 5.0), bw(r#"/media/images/[^"'\s]+\.(?:fill|width|height|max|min)-\d+"#, 5.0)],
        ..NONE
    }),
    entry("Zid", Signature {
        body: &[bw(r"\bmedia\.zid\.store/", 5.0)],
        ..NONE
    }),
    entry("Shoprenter", Signature {
        body: &[bw(r"\bcdn\.shoprenter\.hu/", 5.0)],
        ..NONE
    }),
    entry("SIDEARM Sports", Signature {
        body: &[bw(r"\bsidearmComponents\b", 7.0), bw(r"\bsidearm-icon\b", 7.0), bw(r"\bsidearm\.nextgen\.sites", 7.0)],
        ..NONE
    }),
    entry("novomind iSHOP", Signature {
        body: &[bw(r"\bishop_javascript_evaluation_result\b", 5.0), bw(r"/ishop-api/events/", 5.0), bw(r"\bishop\.api\.management\.", 5.0)],
        ..NONE
    }),
    // A Centra storefront is built by a headless CMS whose CDN host weighs 1: for
    // a store visitor the commerce platform matters more
    entry("Centra", Signature {
        body: &[bw(r"\bcentracdn\.net/", 2.0)],
        ..NONE
    }),
    entry("KQS.store", Signature {
        body: &[b(r"\bkqs_off\(\)"), b(r"\bkqs-iks\b")],
        ..NONE
    }),
    entry("CCV Shop", Signature {
        body: &[b(r"/Plugins/jQuery/css/website/")],
        ..NONE
    }),
    entry("MakeShop", Signature {
        body: &[b(r"\bmakeshop(?:-multi-images)?\.akamaized\.net/"), b(r"\bMakeShopChildCategory\b")],
        ..NONE
    }),
    // The Ecwid widget is embedded in other sites, and there the answer stays
    // their engine: the entry sits below the engines and loses ties to them
    entry("Ecwid", Signature {
        body: &[b(r"\becwid_(?:html|body)\b"), b(r"app\.ecwid\.com/script\.js")],
        ..NONE
    }),
    // The `shared-assets/components/` path on THG's CDN gives MODX one point
    entry("THG Ingenuity", Signature {
        body: &[bw(r"\bstatic\.thcdn\.com/", 2.0)],
        ..NONE
    }),
    // EasyStore runs on Laravel, LocomotiveCMS and Sharetribe on Rails, Cart.com,
    // Smartstore and Mura CMS send ASP.NET headers: the feature weighs more than
    // the base
    entry("EasyStore", Signature {
        body: &[bw(r"\bEasyStore\.Event\.dispatch\(", 5.0), bw(r"\beasystore\.co/\d+/themes/", 5.0)],
        ..NONE
    }),
    entry("LocomotiveCMS", Signature {
        body: &[bw(r"/assets/locomotive/", 5.0)],
        ..NONE
    }),
    entry("Sharetribe", Signature {
        body: &[bw(r"\bsharetribe\.com/images/", 5.0), bw(r"\bsharetribe_promo", 5.0)],
        ..NONE
    }),
    entry("Cart.com", Signature {
        body: &[bw(r"\bamericommerce\.com/", 7.0), bw(r"/store/(?:AddToCart|Search|login)\.aspx", 7.0), bw(r"\bAmeriCommerce-powered-by", 7.0)],
        ..NONE
    }),
    entry("Smartstore", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Smartstore", 7.0), bw(r"\bSmartstore\.cmp\.", 7.0)],
        ..NONE
    }),
    entry("Mura CMS", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Mura CMS", 7.0)],
        ..NONE
    }),
    entry("Homestead", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Homestead")],
        ..NONE
    }),
    entry("Sellingo", Signature {
        body: &[b(r"\bsellingo/cookieconsent/"), b(r"\bsellingo-footer-\d")],
        ..NONE
    }),
    entry("HCL Digital Experience", Signature {
        body: &[b(r"/wps/(?:portal|contenthandler|wcm)\b")],
        ..NONE
    }),
    entry("Koken", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Koken"), b(r"\bkoken-internal\b")],
        ..NONE
    }),
    // Sites migrated from WordPress keep image links into `/wp-content/uploads`:
    // the builder weighs more than that trace
    entry("Phoenix Site", Signature {
        body: &[bw(r"\bphoenixsite\.nl/pageomatic/", 2.0)],
        ..NONE
    }),
    entry("SummerCart", Signature {
        body: &[b(r"\bsummercart\.(?:banners|dynamic_page|rss)\b")],
        ..NONE
    }),
    entry("SoteShop", Signature {
        body: &[b(r"/images/frontend/theme/")],
        ..NONE
    }),
    entry("PinnacleCart", Signature {
        body: &[b(r"\bcontent/cache/skins/")],
        ..NONE
    }),
    entry("Storeden", Signature {
        body: &[b(r"\bstoreden\.(?:net|com)/themes/"), b(r"\bstoredenrequestdigitalinvoice")],
        ..NONE
    }),
    entry("ATSHOP", Signature {
        body: &[b(r"\batshop\.io/bundle/")],
        ..NONE
    }),
    entry("Mixin", Signature {
        body: &[b(r"\bmixin_hash_id\b"), b(r"\bmixin-get-params-compatibility\b")],
        ..NONE
    }),
    entry("SpotHopper", Signature {
        body: &[b(r"/spothopper/image/fetch/")],
        ..NONE
    }),
    entry("Cosmic", Signature {
        body: &[b(r"\b(?:cdn|imgix)\.cosmicjs\.com/")],
        ..NONE
    }),
    entry("Funnelish", Signature {
        body: &[b(r"\bfunnelish\.com/\d+/\d+/")],
        ..NONE
    }),
    // Contensis, BSmart, GrandNode and Menufy send ASP.NET headers, Podia runs on
    // Rails: the platform feature weighs more than the base. Contensis also
    // outweighs AEM: nottingham.ac.uk renders the page with Contensis and takes
    // the header from AEM
    entry("Contensis", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Contensis", 7.0)],
        ..NONE
    }),
    entry("BSmart", Signature {
        body: &[bw(r"\bbsmart_style=", 7.0), bw(r"/bsmart/bsmartstyles/", 7.0)],
        ..NONE
    }),
    entry("GrandNode", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+grandnode", 7.0)],
        ..NONE
    }),
    entry("Menufy Website", Signature {
        body: &[bw(r"\bmenufy-footer\b", 7.0)],
        ..NONE
    }),
    entry("Podia", Signature {
        body: &[bw(r"\b(?:cdn|content)\.podia\.com/", 5.0)],
        ..NONE
    }),
    entry("Halo", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Halo \d", 5.0), bw(r#"content="Halo \d[^"]*" name="?generator"#, 5.0)],
        ..NONE
    }),
    // A Spryker store takes page content from Bloomreach or Neos, whose traces
    // weigh 1: for a store visitor the commerce platform matters more
    entry("Spryker", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+spryker", 2.0)],
        ..NONE
    }),
    entry("4Partners CMS", Signature {
        body: &[b(r"/atlas/build-new/")],
        ..NONE
    }),
    entry("Convertri", Signature {
        body: &[b(r"\bconvertri\.imgix\.net/"), b(r"\bconvertri-script-consent\b")],
        ..NONE
    }),
    entry("Emergent", Signature {
        body: &[b(r"\bemergent\.sh/scripts/emergent-main")],
        ..NONE
    }),
    entry("Future Shop", Signature {
        body: &[b(r"\br2\.future-shop\.jp/fs\.")],
        ..NONE
    }),
    entry("Jumpseller", Signature {
        body: &[b(r"\b(?:cdnx|assets|images)\.jumpseller\.com/")],
        ..NONE
    }),
    entry("LogiCommerce", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+LogiCommerce"), b(r"\blogicommerce\.cloud/")],
        ..NONE
    }),
    entry("Prepr", Signature {
        body: &[b(r"\.(?:cdn|stream)\.prepr\.io/")],
        ..NONE
    }),
    // The Quick.Cart license forbids removing the vendor link, and the template
    // warns about it in a comment
    entry("Quick.Cart", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Quick\.Cart"), b(r#"HIDE LINK "Shopping cart by Quick\.Cart""#)],
        ..NONE
    }),
    entry("Quickbutik", Signature {
        body: &[b(r"\b(?:cdn|storage)\.quickbutik\.com/")],
        ..NONE
    }),
    entry("Sana Commerce", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Sana Commerce"), b(r"\bsana\.cloud/")],
        ..NONE
    }),
    entry("Scrivito", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Scrivito"), b(r"\bscrivito-prerendering-obj-id=")],
        ..NONE
    }),
    entry("TomatoCart", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+TomatoCart")],
        ..NONE
    }),
    entry("WebZi", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Webzi\.ir"), b(r"\bwebzi\.ir/static/")],
        ..NONE
    }),
    entry("microCMS", Signature {
        body: &[b(r"\bmicrocms-assets\.io/")],
        ..NONE
    }),
    // AIMEOS and Shuttle run on Laravel, Akinon on Django, HighStore, Milestone,
    // SellersCommerce and Ticimax send ASP.NET headers: the feature weighs no less
    // than the base
    entry("AIMEOS", Signature {
        body: &[bw(r"<meta[^>]+application-name[^>]+Aimeos", 5.0), bw(r"/aimeos\.css\b", 5.0)],
        ..NONE
    }),
    entry("Shuttle", Signature {
        body: &[bw(r"\bshuttle-storage\.s3\.amazonaws\.com/", 5.0)],
        ..NONE
    }),
    entry("Akinon", Signature {
        body: &[bw(r"\bakinoncloud\.com/", 5.0), bw(r"\bakinoncdn\.com", 5.0)],
        ..NONE
    }),
    entry("HighStore", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+HighStore", 7.0)],
        ..NONE
    }),
    entry("Milestone CMS", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Milestone CMS", 7.0)],
        ..NONE
    }),
    entry("SellersCommerce", Signature {
        body: &[bw(r"\bsellerscommerce\.com/scassets/", 7.0)],
        ..NONE
    }),
    entry("Ticimax", Signature {
        body: &[bw(r"\bticimax\.cloud/", 7.0)],
        ..NONE
    }),
    // Frontastic serves a storefront on top of Contentful, whose CDN host weighs 1
    entry("Frontastic", Signature {
        body: &[bw(r"\bfrontastic_canonical_route\b", 2.0), bw(r"\bfrontastic\.frontend\.", 2.0)],
        ..NONE
    }),
    // The generator `Hostinger AI Builder` also matches the Hostinger Website
    // Builder feature, which is listed higher
    entry("Hostinger Horizons", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Hostinger AI Builder", 2.0)],
        ..NONE
    }),
    entry("Adwos CMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Adwos CMS")],
        ..NONE
    }),
    entry("AlvandCMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+AlvandCMS")],
        ..NONE
    }),
    entry("AsciiDoc", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+AsciiDoc")],
        ..NONE
    }),
    entry("Brownie", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Brownie"), b(r"/brownie/scripts/")],
        ..NONE
    }),
    entry("Caisy", Signature {
        body: &[b(r"\bassets\.caisy\.io/")],
        ..NONE
    }),
    entry("CloudSuite", Signature {
        body: &[b(r"\bs3-cdn\.cloudsuite\.com/")],
        ..NONE
    }),
    entry("CoreMedia", Signature {
        body: &[b(r#"<meta name="coremedia:content-id""#)],
        ..NONE
    }),
    entry("Easy Orders", Signature {
        body: &[b(r"\beasyorders\.shop/_next/")],
        ..NONE
    }),
    entry("FatherShops", Signature {
        body: &[b(r"\bstatic\.myfathershops\.com/")],
        ..NONE
    }),
    entry("Fynd Platform", Signature {
        body: &[b(r"\bcdn\.fynd\.com/v2/")],
        ..NONE
    }),
    entry("Griddo", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Griddo")],
        ..NONE
    }),
    entry("Kreatio", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Kreatio"), b(r"\bkreatio\.net/")],
        ..NONE
    }),
    entry("MyOnlineStore", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Mijnwebwinkel"), b(r"\bmyonlinestore\.eu/")],
        ..NONE
    }),
    entry("Mysitefy", Signature {
        body: &[b(r"\bmysitefy\.com/img/")],
        ..NONE
    }),
    entry("OnUniverse", Signature {
        body: &[b(r"\bonuniverse-assets\.imgix\.net/")],
        ..NONE
    }),
    entry("Shopcada", Signature {
        body: &[b(r"\bshopcada-grid-")],
        ..NONE
    }),
    entry("Simplo7", Signature {
        body: &[b(r"\bsimplo7\.net/static/")],
        ..NONE
    }),
    entry("SmartWeb", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+SmartWeb"), b(r"\bsmartweb-static\.com/")],
        ..NONE
    }),
    entry("Swell", Signature {
        body: &[b(r"\bcdn\.swell\.store/")],
        ..NONE
    }),
    entry("The Church Co", Signature {
        body: &[b(r"\bthechurchcoassets\.com/"), b(r"\bthechurchco-production\.")],
        ..NONE
    }),
    entry("Vendre", Signature {
        body: &[b(r"\bvendreCartUpdate\b")],
        ..NONE
    }),
    entry("WebsPlanet", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Websplanet")],
        ..NONE
    }),
    entry("Wikinggruppen", Signature {
        body: &[b(r"<!-- WIKINGGRUPPEN \d")],
        ..NONE
    }),
    entry("Xanario", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+xanario")],
        ..NONE
    }),
    entry("Zoho Commerce", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Zoho Commerce"), b(r"\bzohoecommerce\.com/")],
        ..NONE
    }),
    entry("eSyndiCat", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+eSyndiCat")],
        ..NONE
    }),
    entry("phpSQLiteCMS", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+phpSQLiteCMS")],
        ..NONE
    }),
    entry("Congressus", Signature {
        body: &[b(r"\bcongressus-static-backend\.s3\.amazonaws\.com/")],
        ..NONE
    }),
    entry("Dr. Leonardo", Signature {
        body: &[b(r"\bdrleonardo\.com\.(?:templates|dev\.misc)/")],
        ..NONE
    }),
    // Lede serves a Next.js edition on top of its own WordPress, which scores one
    // point from images at `lede-admin.<domain>/wp-content/`
    entry("Lede", Signature {
        body: &[bw(r"\blede_byline_", 2.0), bw(r"\blede-admin\.", 2.0)],
        ..NONE
    }),
    // Bag, Blutui, InnoShop, Sociavore, Twsaa and simploCMS run on Laravel, Nimbu
    // and Shopmaker on Rails, Podpage on Django, ForoshGostar on nopCommerce, Webx
    // sends ASP.NET headers, and Storearmy October's paths: the feature weighs no
    // less than the base. Packman stores score Framer one point; Packman weighs 2
    entry("Abicart", Signature {
        body: &[b(r"\bcdn\.abicart\.com/")],
        ..NONE
    }),
    entry("AboutMyClinic", Signature {
        body: &[b(r"\bstatic\.aboutmyclinic\.com/")],
        ..NONE
    }),
    entry("Bag", Signature {
        body: &[bw(r"\bcdn\.shantaweb\.com/", 5.0)],
        ..NONE
    }),
    entry("Bigshop", Signature {
        body: &[b(r"\bassetspublic\.bigshop\.com\.br/"), b(r"\bstatic\.bigshop\.com\.br/")],
        ..NONE
    }),
    entry("Blutui", Signature {
        body: &[bw(r"\bcdn\.blutui\.com/", 5.0)],
        ..NONE
    }),
    entry("Comarch e-Sklep", Signature {
        body: &[b(r"\bstatic\.comarchesklep\.pl/")],
        ..NONE
    }),
    entry("Crystallize", Signature {
        body: &[b(r"\bmedia\.crystallize\.com/")],
        ..NONE
    }),
    entry("Dukany", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Dukany"), b(r"\borderapi\.dukany\.io")],
        ..NONE
    }),
    entry("Ferret One", Signature {
        body: &[b(r"\bferret-one\.akamaized\.net/")],
        ..NONE
    }),
    entry("Florist Touch", Signature {
        body: &[b(r"\bclientassets\.floristtouch\.co\.uk/"), b(r"\bstatic\.floristtouch\.com/")],
        ..NONE
    }),
    entry("ForoshGostar", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+foroshGostar", 5.0)],
        ..NONE
    }),
    entry("InnoShop", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+InnoShop", 5.0)],
        ..NONE
    }),
    entry("Jibres", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Jibres")],
        ..NONE
    }),
    entry("Kartmax", Signature {
        body: &[b(r"\bpictures\.kartmax\.in/")],
        ..NONE
    }),
    entry("LiveBooks", Signature {
        body: &[b(r"\bstatic\.livebooks\.com/")],
        ..NONE
    }),
    entry("LiveSite", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+liveSite")],
        ..NONE
    }),
    entry("LogaTech", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+LogaTech")],
        ..NONE
    }),
    entry("Mattaki", Signature {
        body: &[b(r"\bcdn\.mattaki\.com/")],
        ..NONE
    }),
    entry("MGPanel", Signature {
        body: &[b(r"\bmgpanel-v\d+\.s3\.[a-z0-9.-]*amazonaws\.com/")],
        ..NONE
    }),
    entry("NagaCommerce", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+NagaCommerce"), b(r"\bassets\.nagacommerce\.com/")],
        ..NONE
    }),
    entry("Nimbu", Signature {
        body: &[bw(r"\b(?:cdn|static)\.nimbu\.io/", 5.0)],
        ..NONE
    }),
    entry("Orbit Commerce", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Orbit Commerce")],
        ..NONE
    }),
    entry("Packman", Signature {
        body: &[bw(r"\bcdn\.packman\.(?:ai|app)/", 2.0)],
        ..NONE
    }),
    entry("Pixelesq", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Pixelesq")],
        ..NONE
    }),
    entry("Plugo", Signature {
        body: &[b(r"\bapi\.plugo\.world/v1/shop/")],
        ..NONE
    }),
    entry("Podpage", Signature {
        body: &[bw(r"\b(?:img|static)\.podpage\.com/", 5.0)],
        ..NONE
    }),
    entry("Portal", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Portal Site Builder"), b(r"\bstatic\.portal\.ir/")],
        ..NONE
    }),
    entry("Poski", Signature {
        body: &[b(r"\bcdn\.poski\.com/")],
        ..NONE
    }),
    entry("Publishrr", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Publishrr")],
        ..NONE
    }),
    entry("Qukasoft", Signature {
        body: &[b(r"\bcdn\.qukasoft\.com/")],
        ..NONE
    }),
    entry("Scayle", Signature {
        body: &[b(r"\.checkout\.api\.scayle\.cloud/")],
        ..NONE
    }),
    entry("Selless", Signature {
        body: &[b(r"\bcdn\.selless\.us/")],
        ..NONE
    }),
    entry("ShopFactory", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+ShopFactory")],
        ..NONE
    }),
    entry("Shopblocks", Signature {
        body: &[b(r"\bstatic\.shopblocks\.com/")],
        ..NONE
    }),
    entry("Shopmaker", Signature {
        body: &[bw(r"\bstatic\.shopmaker\.com/", 5.0)],
        ..NONE
    }),
    entry("ShoutCMS", Signature {
        body: &[b(r"\bassets-web\d+\.shoutcms\.net/")],
        ..NONE
    }),
    entry("Silex", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Silex v\d")],
        ..NONE
    }),
    entry("Sixshop", Signature {
        body: &[b(r"\bstatic\.sixshop\.com/")],
        ..NONE
    }),
    entry("Sketchanet", Signature {
        body: &[b(r"\bcors\.sketchanet\.com/")],
        ..NONE
    }),
    entry("Sociavore", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Sociavore", 5.0)],
        ..NONE
    }),
    entry("Solusquare OmniCommerce Cloud", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Solusquare")],
        ..NONE
    }),
    entry("Storearmy", Signature {
        body: &[bw(r"\bcdn\.storearmy\.com/", 5.0)],
        ..NONE
    }),
    entry("Storeino", Signature {
        body: &[b(r"\bstoreno\.b-cdn\.net/")],
        ..NONE
    }),
    entry("TakeDrop", Signature {
        body: &[b(r"\bmain\.takedropstorage\.com/")],
        ..NONE
    }),
    entry("Tokeet", Signature {
        body: &[b(r"\bcdn\.tokeet\.com/")],
        ..NONE
    }),
    entry("Twsaa", Signature {
        body: &[bw(r"\bcdn\.twsaa\.com/", 5.0)],
        ..NONE
    }),
    entry("VerseOne", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+VerseOne")],
        ..NONE
    }),
    entry("Webready", Signature {
        body: &[b(r"\bcdn\.usewebready\.com/")],
        ..NONE
    }),
    entry("Webx", Signature {
        body: &[bw(r"\bstatic\d*\.webx\.pk/", 7.0)],
        ..NONE
    }),
    entry("WiziShop", Signature {
        body: &[b(r"\bsentry\.wizishop\.com/")],
        ..NONE
    }),
    entry("Yclas", Signature {
        body: &[b(r"\byclas\.nyc3\.cdn\.digitaloceanspaces\.com/")],
        ..NONE
    }),
    entry("iWiki", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+iWink CMS")],
        ..NONE
    }),
    entry("immediaCMS", Signature {
        body: &[b(r"\bimmediac\.blob\.core\.windows\.net/")],
        ..NONE
    }),
    entry("simploCMS", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+simploCMS", 5.0)],
        ..NONE
    }),
    entry("Behzi", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Behzi")],
        ..NONE
    }),
    entry("Bricksite", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Bricksite")],
        ..NONE
    }),
    // Saffire sends ASP.NET headers, CommentSold runs on Laravel. Single traces of
    // WordPress and OpenCart occur on Assemble and Brushd sites
    entry("Alboom Prosite", Signature {
        body: &[b(r"\bbifrost\.alboompro\.com/static"), b(r"\bcdn-cp\.alboompro\.com/")],
        ..NONE
    }),
    entry("Brushd", Signature {
        body: &[bw(r"\bassets\.brushd\.co/", 2.0)],
        ..NONE
    }),
    entry("Maglr", Signature {
        body: &[b(r"\bsystem\.maglr\.com/"), b(r"\bdata\.maglr\.com/")],
        ..NONE
    }),
    entry("Saffire", Signature {
        body: &[bw(r"\bcdn\.saffire\.com/theme-files", 7.0)],
        ..NONE
    }),
    entry("Assemble", Signature {
        body: &[bw(r"\bcdn\.assemble\.me/", 2.0)],
        ..NONE
    }),
    entry("Cococart", Signature {
        body: &[b(r"\bcdn\.cococart\.co/")],
        ..NONE
    }),
    entry("CommentSold", Signature {
        body: &[bw(r"\b(?:s3|cdn)\.commentsold\.com/", 5.0)],
        ..NONE
    }),
    entry("Control", Signature {
        body: &[b(r"\bcdn\.cntrl\.site/"), b(r"<meta[^>]+generator[^>]+cntrl\.site")],
        ..NONE
    }),
    entry("Webzie", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+Webzie")],
        ..NONE
    }),
    // Mabisy, OrderPort and Sqwiz send ASP.NET headers, Zepio and Sharing run on
    // Laravel
    entry("Contentder", Signature {
        body: &[b(r"\bcdn\.contentder\.com/")],
        ..NONE
    }),
    entry("Inblog", Signature {
        body: &[b(r"\bimage\.inblog\.dev/"), b(r"<meta[^>]+generator[^>]+inblog")],
        ..NONE
    }),
    entry("Justo", Signature {
        body: &[b(r"\bwebcdn\.getjusto\.com/"), b(r"\btofuu\.getjusto\.com/")],
        ..NONE
    }),
    entry("Mabisy", Signature {
        body: &[bw(r"<meta[^>]+generator[^>]+Mabisy", 7.0)],
        ..NONE
    }),
    entry("OrderPort", Signature {
        body: &[bw(r"\borderport-webstore\.b-cdn\.net/", 7.0)],
        ..NONE
    }),
    entry("QUV", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+QUV Homepage Builder"), b(r"\bcdn\.quv\.kr/")],
        ..NONE
    }),
    entry("Qshop", Signature {
        body: &[b(r"\bcdn\.qshop\.ai/"), b(r"\bqlog-sdk\.qshop\.ai/")],
        ..NONE
    }),
    entry("ShopGate", Signature {
        body: &[b(r"\bdata\.shopgate\.com/"), b(r"\bimg-cdn\.shopgate\.com/")],
        ..NONE
    }),
    entry("Shoprocket", Signature {
        body: &[b(r"\b(?:cdn|img)\.shoprocket\.io/")],
        ..NONE
    }),
    entry("StackerHQ", Signature {
        body: &[b(r"\bcdn\.stackerhq\.com/")],
        ..NONE
    }),
    entry("Weezbe", Signature {
        body: &[b(r"\bstatic\.weezbe\.com/"), b(r"\bmedias\.weezbe\.com/")],
        ..NONE
    }),
    entry("Zepio", Signature {
        body: &[bw(r"\bcdn\.zepio\.io/", 5.0)],
        ..NONE
    }),
    entry("eDokan", Signature {
        body: &[b(r"\bstatic\.edokan\.co/"), b(r"\bcdn\.edokan\.co/")],
        ..NONE
    }),
    entry("Sharing", Signature {
        body: &[bw(r#"generator"\s+content="Sharing""#, 5.0)],
        ..NONE
    }),
    entry("Siter", Signature {
        body: &[b(r"\b(?:cdn|api)\.siter\.io/assets/")],
        ..NONE
    }),
    entry("Sqwiz", Signature {
        body: &[bw(r"/userfiles/sqwiz_", 7.0)],
        ..NONE
    }),
    entry("BBT bCube NX", Signature {
        body: &[b(r"<meta[^>]+generator[^>]+BBT bCube")],
        ..NONE
    }),
    entry("Plentymarkets", Signature {
        body: &[bw(r"cdn[0-9]+\.plenty(markets|one)\.com/", 2.0), b(r"<meta[^>]+generator[^>]+plentymarkets"), b(r"\bplenty-shop-cookie")],
        ..NONE
    }),
    entry("React", Signature {
        body: &[bw(r"\bdata-reactroot", 2.0), bw(r#"id="root""#, 1.0)],
        ..NONE
    }),
    // Vue's scoped marker is `data-v-` followed by eight hex digits. Without them
    // the feature matches someone else's attribute: `data-v-align="center"` on
    // RapidWeaver
    entry("Vue", Signature {
        body: &[b(r"\bdata-v-[0-9a-f]{8}")],
        ..NONE
    }),
    entry("Angular", Signature {
        body: &[b(r"\bng-app"), b(r"\bng-version")],
        ..NONE
    }),
    // Only the field name `authenticity_token` is specific to Rails. The
    // `csrf-param` and `csrf-token` pair is written by Yii and hand-made backends
    // too: www.rbc.ru has `csrf_token` there
    entry("Ruby on Rails", Signature {
        body: &[bw(r#"content="authenticity_token""#, 2.0), bw(r#"name="authenticity_token""#, 2.0)],
        ..NONE
    }),
    entry("ASP.NET", Signature {
        body: &[bw(r"\b__VIEWSTATE", 3.0)],
        headers: &[
            ("x-aspnet-version", &[h(r".+")]),
            ("x-powered-by", &[h(r"ASP\.NET")]),
        ],
    }),
    entry("Vite", Signature {
        body: &[b(r#"type="module"\s+src="/@vite"#), b(r#"type="module"\s+src="/src/"#)],
        ..NONE
    }),
];

/// Presentation-layer CMSs and frameworks: a site may run them on top of any
/// backend (headless), so they do not compete with the backend for its single
/// place in the result of `select_winners`.
pub(crate) const FRONTEND_LAYER: &[&str] = &[
    "Next.js", "Nuxt.js", "Astro", "Svelte", "Gatsby", "React", "Vue", "Angular", "Vite",
];

pub(crate) fn is_frontend(name: &str) -> bool {
    FRONTEND_LAYER.contains(&name)
}

/// The score of one CMS or framework, in detection order. The order decides
/// ties (`select_winners` sorts stably), which is why scores travel as a list,
/// not a map.
#[derive(Debug, Clone, PartialEq)]
pub struct CmsScore {
    pub name: String,
    pub score: f64,
}

/// Rejects an answer based on one weak feature: the theme path
/// `/themes/<theme>/assets/` weighs 0.5 for October and Winter and appears on
/// hundreds of unrelated sites, so without a threshold any site with no other
/// features became October.
const MIN_WINNER_SCORE: f64 = 1.0;

/// Applies the "one site, one engine" rule: among competing engines only one
/// wins (the highest score; on a tie, the first in detection order), and a
/// presentation layer is added as a separate entry on top of it (a headless
/// pair such as Next.js + WordPress). A candidate scoring below 1 never wins.
#[must_use]
pub fn select_winners(scores: &[CmsScore]) -> Vec<String> {
    let mut ranked = scores.to_vec();
    sort_by_score_desc(&mut ranked);

    let (mut backend, mut frontend) = (None, None);
    for s in ranked {
        if s.score < MIN_WINNER_SCORE {
            continue;
        }
        let slot = if is_frontend(&s.name) {
            &mut frontend
        } else {
            &mut backend
        };
        if slot.is_none() {
            *slot = Some(s);
        }
    }

    let mut result: Vec<CmsScore> = backend.into_iter().chain(frontend).collect();
    sort_by_score_desc(&mut result);
    result.into_iter().map(|s| s.name).collect()
}

fn sort_by_score_desc(scores: &mut [CmsScore]) {
    scores.sort_by(|a, b| b.score.total_cmp(&a.score));
}
