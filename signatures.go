package cmslens

import (
	"regexp"
	"sort"
)

// sigItem is one feature: a regular expression and the weight of a match.
// Weights are not always whole (October/Winter CMS use 0.5), so scores are
// float64.
type sigItem struct {
	re     *regexp.Regexp
	weight float64
}

// signature is the set of features of one CMS or framework: in the page body
// and in the response headers. A header weighs more by default, because it is
// harder to fake than markup.
type signature struct {
	body    []sigItem
	headers map[string][]sigItem
}

const (
	bodyDefaultWeight   = 1
	headerDefaultWeight = 2
)

/*
A short word-like feature is written with a leading `\b`. Without the boundary
the fragment matches at the end of someone else's word, and on Russian sites
that is everyday life rather than a rare case: transliteration turns
`derevo-alyuminievaya` into a match for `evo-`, and someone's path
`WEEKEND_Online_Site/` into a match for `_site/`. No boundary on the right: what
follows there is almost always meaningful (`hubspotusercontent`,
`bx-core-window`).
*/
func ci(pattern string) *regexp.Regexp { return regexp.MustCompile("(?i)" + pattern) }

func b(pattern string) sigItem                  { return sigItem{re: ci(pattern), weight: bodyDefaultWeight} }
func bw(pattern string, weight float64) sigItem { return sigItem{re: ci(pattern), weight: weight} }
func h(pattern string) sigItem                  { return sigItem{re: ci(pattern), weight: headerDefaultWeight} }
func hw(pattern string, weight float64) sigItem { return sigItem{re: ci(pattern), weight: weight} }

// cmsEntry keeps the name next to the signature as an explicit field rather
// than a map key: the order of this list decides who wins a tie (SelectWinners
// sorts stably).
type cmsEntry struct {
	name string
	sig  signature
}

var cmsSignatures = []cmsEntry{
	{"1C-Bitrix", signature{
		body: []sigItem{b(`bitrix/js/`), b(`bitrix/templates/`), b(`bitrix/css/`), b(`BX\.message`), b(`\bbx-core`), b(`\bbx-panel`), b(`/upload/uf/`)},
		headers: map[string][]sigItem{
			"set-cookie":    {h(`BITRIX_SM_`)},
			"x-powered-cms": {h(`Bitrix Site Manager`)},
		},
	}},
	{"WordPress", signature{
		body: []sigItem{b(`/wp-content/`), b(`/wp-includes/`), b(`\bwp-submit`), b(`wp-embed\.min\.js`), b(`<meta[^>]+generator[^>]+WordPress`)},
		headers: map[string][]sigItem{
			// The `x-pingback` header alone is not WordPress: pingback is a general blog
			// protocol, and Typecho announces it with its own `/action/xmlrpc`
			"x-pingback": {h(`xmlrpc\.php`)},
			"link":       {h(`/wp-json/`)},
			"set-cookie": {h(`wp-settings`), h(`wordpress_`)},
		},
	}},
	// `tilda.ws` is not a feature: it is the address of any Tilda project, and
	// links to it sit in the menus of other sites (3lp.me on Eleventy). From the
	// CDN only scripts and styles count: images from it are embedded in articles
	// on other engines too (promo-blog.ru on Next.js)
	{"Tilda", signature{
		body: []sigItem{b(`tildacdn\.(com|re)/(js|css)/`), b(`tilda3\.0\.min\.js`), b(`tilda-grid\.css`), b(`id="allrecords"`), b(`<meta[^>]+generator[^>]+Tilda`)},
	}},
	// The `assets/components/` directory counts only from the site root (MODX
	// writes it relative or from `/`): a nested path occurs in third-party plugins
	// and CDNs (`shared-assets/components/` at THG), and a root path on a foreign
	// host at Envato
	{"MODX", signature{
		body: []sigItem{b(`["'(=]/?assets/components/`), b(`assets/templates/`), b(`\bMODX_MEDIA`), b(`\[\[\+`)},
	}},
	// `evo-` cannot stand here without a word boundary: the fragment matches at
	// the end of someone else's word, and transliteration ("derevo-alyuminievaya",
	// "dizanalevo-m") declared Evolution CMS on sites that do not run it at all.
	// It is recognised by what belongs to the CMS itself: the snippets directory
	// (MODX Revolution does not have it) and the engine's name in the markup
	{"Evolution CMS", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Evolution CMS`), b(`assets/templates/`), b(`assets/snippets/`), b(`\bevolutioncms`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bevo[a-zA-Z0-9]{5,}`)}},
	}},
	{"OpenCart", signature{
		body:    []sigItem{b(`catalog/view/javascript/`), b(`catalog/view/theme/`), b(`index\.php\?route=checkout/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`OCSESSID`)}},
	}},
	{"Joomla", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Joomla`), b(`/media/system/js/`), b(`/media/jui/`)},
	}},
	{"Drupal", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Drupal`), b(`/sites/default/files/`), b(`Drupal\.settings`), b(`\bdata-drupal-selector`)},
	}},
	// A bare `cdn.shopify.com` is not a feature: www.framer.com loads a consent
	// script from it too. Store files live under `/s/files/` or are served from
	// the store's own domain under `/cdn/shop/`
	{"Shopify", signature{
		body:    []sigItem{b(`cdn\.shopify\.com/s/files/`), b(`/cdn/shop/`), b(`Shopify\.shop`), b(`Shopify\.theme`), b(`\bshopify-section`)},
		headers: map[string][]sigItem{"x-shopid": {h(`.+`)}, "powered-by": {h(`Shopify`)}},
	}},
	{"Wix", signature{
		body: []sigItem{b(`static\.wixstatic\.com`), b(`<meta[^>]+generator[^>]+Wix\.com`)},
	}},
	{"Webflow", signature{
		body: []sigItem{b(`\bdata-wf-page`), b(`\bdata-wf-site`), b(`webflow\.js`)},
	}},
	{"Squarespace", signature{
		body: []sigItem{b(`static1\.squarespace\.com`), b(`Squarespace\.VERSION`)},
	}},
	{"Magento", signature{
		body: []sigItem{b(`text/x-magento-init`), b(`Mage\.Cookies`)},
	}},
	// The word PrestaShop itself is not a feature: hosting providers mention it in
	// their one-click install ads (gandi.net)
	{"PrestaShop", signature{
		body:    []sigItem{bw(`var prestashop\s*=`, 2)},
		headers: map[string][]sigItem{"set-cookie": {h(`PrestaShop`)}},
	}},
	{"InSales", signature{
		body: []sigItem{b(`\binsales\.ru`), b(`\bstatic-insales`)},
	}},
	{"DataLife Engine", signature{
		body:    []sigItem{b(`engine/classes/js/`), b(`<meta[^>]+generator[^>]+DataLife Engine`), b(`\bdle_root`), b(`\bdle_admin`)},
		headers: map[string][]sigItem{"set-cookie": {h(`dle_user_id`), h(`dle_password`), h(`dle_hash`)}},
	}},
	{"UMI.CMS", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+UMI\.CMS`), b(`/js/client/umi`), b(`<meta[^>]+author[^>]+Umisoft`), b(`xmlns:umi`), b(`umi\.ru`), b(`umi-cms\.ru`)},
	}},
	{"NetCat", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+NetCat`), b(`\bnetcat_files`), bw(`/netcat/`, 3)},
	}},
	{"CS-Cart", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+CS-Cart`), b(`\bcm-dialog-auto-size`), b(`Tygh\.`)},
	}},
	{"HostCMS", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+HostCMS`), b(`\bhostcms_files`), b(`\bhostcms`)},
	}},
	{"Amiro.CMS", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Amiro`), b(`\bamiro\.ru`)},
	}},
	{"DIAFAN.CMS", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+DIAFAN\.CMS`), b(`\bdiafan`)},
	}},
	{"Moguta.CMS", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Moguta\.CMS`), b(`\bmg-core`)},
	}},
	{"Shop-Script", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Shop-Script`), b(`\bwa-data`), b(`\bwa-content`)},
	}},
	{"VamShop", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+VamShop`), b(`\bvamshop`)},
	}},
	// A SaaS platform for restaurants and delivery: the platform builds the site
	// and Next.js renders it. Recognised by its CDN and API domains, which sit on
	// every client page and nowhere else. A bare `starterapp.ru` weighs less:
	// sites that merely link to the platform write it too
	{"StarterApp", signature{
		body: []sigItem{bw(`cdn\.starterapp\.(ru|co)`, 3), bw(`api\.starterapp\.ru`, 3), b(`\bstarterapp\.ru`)},
	}},
	// The word `ucoz` is not a feature: links to an old uCoz site appear on other
	// people's pages too (bolknote.ru)
	{"uCoz", signature{
		body:    []sigItem{b(`="/\.s/src/`), b(`\.ucoz\.net/`), b(`\buid\.me`)},
		headers: map[string][]sigItem{"set-cookie": {h(`uCoz=`)}},
	}},
	{"uKit", signature{
		body: []sigItem{b(`\bukit\.com`), b(`<meta[^>]+generator[^>]+uKit`)},
	}},
	// A right-hand boundary is needed too: `setup.ru` is the start of the
	// unrelated domain `setup.runtipi.io`, and a site on Nextra was declared a
	// Russian site builder
	{"Setup.ru", signature{
		body: []sigItem{b(`\bsetup\.ru\b`)},
	}},
	{"Nethouse", signature{
		body: []sigItem{b(`\bnethouse\.ru`)},
	}},
	{"AdvantShop", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+AdvantShop`), b(`\badvantshop`)},
	}},
	{"Ghost", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Ghost`), b(`\bghost-frontend`)},
	}},
	{"October CMS", signature{
		body: []sigItem{
			bw(`/themes/[^/]+/assets/`, 0.5),
			b(`/modules/system/assets/`),
			b(`/modules/system/assets/js/framework(?:\.combined)?\.js`),
			b(`/plugins/[a-z0-9_-]+/[a-z0-9_-]+/assets/`),
			bw(`/combine/[a-f0-9]+`, 2),
			b(`<meta[^>]+generator[^>]+October`),
		},
		headers: map[string][]sigItem{"set-cookie": {h(`october_session`)}},
	}},
	{"Winter CMS", signature{
		body: []sigItem{
			bw(`/themes/[^/]+/assets/`, 0.5),
			b(`/modules/system/assets/`),
			b(`/modules/system/assets/js/framework(?:\.combined)?\.js`),
			b(`/plugins/[a-z0-9_-]+/[a-z0-9_-]+/assets/`),
			bw(`/combine/[a-f0-9]+`, 2),
			b(`<meta[^>]+generator[^>]+Winter`),
		},
		headers: map[string][]sigItem{"set-cookie": {h(`winter_session`)}},
	}},
	// HubSpot's tracker and forms are put on sites of every engine, so neither
	// `js.hs-scripts.com` nor the word itself is a feature: the feature is what
	// its CMS emits
	{"HubSpot", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+HubSpot`), bw(`/hub_generated/`, 2)},
		headers: map[string][]sigItem{"x-hs-content-id": {h(`.+`)}},
	}},
	{"Next.js", signature{
		body: []sigItem{b(`id="__NEXT_DATA__"`), b(`/_next/static/`), b(`<meta[^>]+generator[^>]+Next\.js`)},
	}},
	{"Nuxt.js", signature{
		body:    []sigItem{b(`id="__NUXT__"`), b(`window\.__NUXT__`), b(`\bnuxt-link`), b(`\bdata-n-head`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bNuxt\b`)}},
	}},
	{"Astro", signature{
		body: []sigItem{b(`<astro-island`), b(`class="astro-[a-zA-Z0-9]{8}"`), b(`<meta[^>]+generator[^>]+Astro`)},
	}},
	// `<meta name="csrf-token">` is written by Rails, Yii, InstantCMS and
	// hand-made backends alike, so it does not name Laravel: on www.rbc.ru it was
	// the only feature
	{"Laravel", signature{
		body:    []sigItem{b(`name="_token"`)},
		headers: map[string][]sigItem{"set-cookie": {h(`XSRF-TOKEN`), h(`laravel_session`)}},
	}},
	{"Django", signature{
		body:    []sigItem{b(`name="csrfmiddlewaretoken"`), b(`\bdjango-admin`)},
		headers: map[string][]sigItem{"set-cookie": {h(`csrftoken`)}},
	}},
	{"Hugo", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Hugo`), b(`powered by Hugo`)},
	}},
	// `_site/` was removed: it is Jekyll's build directory and never appears in
	// served URLs, but it matched inside someone else's path
	// (`WEEKEND_Online_Site/`)
	{"Jekyll", signature{
		body: []sigItem{b(`<meta[^>]+generator[^>]+Jekyll`), b(`Jekyll v`)},
	}},
	{"Gatsby", signature{
		body: []sigItem{b(`id="___gatsby"`), b(`\bgatsby-image`), b(`<meta[^>]+generator[^>]+Gatsby`)},
	}},
	{"Svelte", signature{
		body: []sigItem{b(`class="svelte-[a-zA-Z0-9]{5,8}"`), b(`\b__sveltekit`)},
	}},
	{"Webasyst", signature{
		body: []sigItem{bw(`\bwa-data/`, 3), bw(`\bwa-content/`, 3), b(`<meta[^>]+generator[^>]+Webasyst`)},
	}},
	{"Simpla", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Simpla`)}}},
	{"OkayCMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+OkayCMS`)}}},
	{"ABO.CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+ABO\.CMS`)}}},
	{"SiteEdit", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+SiteEdit`)}}},
	{"ImageCMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+ImageCMS`)}}},
	{"LPgenerator", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+LPgenerator`)}}},
	{"Flexbe", signature{body: []sigItem{b(`\bflexbe\.ru`), b(`\bflexbe\.com`), b(`<meta[^>]+name="flexbe-theme-id"`), b(`window\.flexbe_cli`)}}},
	{"Mottor", signature{body: []sigItem{b(`\bmottor`), b(`\blptrend`)}}},
	{"Craftum", signature{body: []sigItem{b(`\bcraftum\.com`), b(`\bcraftum`)}}},
	{"Tobiz", signature{body: []sigItem{b(`\btobiz\.net`)}}},
	{"Creatium", signature{body: []sigItem{b(`\bcreatium\.io`), b(`assets\.creatium\.io`)}}},
	{"CMS.S3", signature{body: []sigItem{b(`\bcms\.s3`), b(`\bmegagroup\.ru`), b(`="/g/s3/`), b(`="/t/v[0-9]+/`), b(`/g/shop2`), b(`/shared/s3/`)}}},
	// Taptop is built on the Megagroup platform and carries the same paths and
	// tracker as CMS.S3 (up to 3 points), so its generator weighs more
	{"Taptop", signature{body: []sigItem{bw(`<meta[^>]+content="Taptop"[^>]+name="generator"`, 5)}}},
	{"Typo3", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+TYPO3`), b(`\btypo3temp/`), b(`\btypo3conf/`)}}},
	// Since version 9 the engine is called Concrete CMS; the table keeps the old
	// name, which users know better
	{"Concrete5", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+concrete( |5)`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bCONCRETE5=`)}},
	}},
	{"Contao", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Contao`)}}},
	{"Craft CMS", signature{headers: map[string][]sigItem{"x-powered-by": {h(`Craft CMS`)}}}},
	{"Weebly", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Weebly`)}}},
	// Links to blogger.com appear in the share buttons of other sites too: the
	// features are resources served by Blogger itself and its generator written
	// value first
	{"Blogger", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Blogger`), b(`content=['"]blogger['"] name=['"]generator`), b(`\bwww\.blogger\.com/(?:static|dyn-css|openid-server)`), b(`\b(?:resources|widgets)\.blogblog\.com/`)}}},
	{"phpBB", signature{body: []sigItem{b(`<p[^>]+>Powered by.+phpBB`)}}},
	// The engine's name in text is not a feature: the Discourse forum writes it in
	// articles about migrating from XenForo
	{"XenForo", signature{
		body:    []sigItem{b(`js/xenforo/`), bw(`\bdata-xf-init`, 2), b(`<html[^>]+id="XF"`), b(`/js/xf/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bxf_csrf`)}},
	}},
	{"vBulletin", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+vBulletin`)}}},
	{"BigCommerce", signature{body: []sigItem{b(`cdn\d+\.bigcommerce\.com`)}}},
	{"Volusion", signature{body: []sigItem{b(`\bvolusion\.com`)}}},
	{"Umbraco", signature{headers: map[string][]sigItem{"x-umbraco-version": {h(`.+`)}}}},
	{"Strapi", signature{headers: map[string][]sigItem{"x-powered-by": {h(`Strapi`)}}}},
	// A bare `sanity.io` is not a feature: the name appears in the list of data
	// sources on gridsome.org. A site on Sanity pulls its content from Sanity's
	// hosts
	{"Sanity", signature{body: []sigItem{b(`\b(?:cdn|apicdn)\.sanity\.io`), b(`\.api\.sanity\.io`)}}},
	{"Contentful", signature{body: []sigItem{b(`\bcontentful\.com`)}}},
	{"Prismic", signature{body: []sigItem{b(`\bprismic\.io`)}}},
	{"Jimdo", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Jimdo`)}}},
	{"Yola", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Yola`)}}},
	{"Strikingly", signature{body: []sigItem{b(`\bstrikingly\.com`)}}},
	{"Duda", signature{body: []sigItem{b(`\bduda\.co`)}}},
	{"Webnode", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Webnode`)}}},
	{"Sitecore", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Sitecore`)}}},
	{"Kentico", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Kentico`)}}},
	{"Liferay", signature{headers: map[string][]sigItem{"liferay-portal": {h(`.+`)}}}},
	{"Zen Cart", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Zen Cart`)}}},
	{"osCommerce", signature{body: []sigItem{b(`\boscommerce`)}}},
	{"ExpressionEngine", signature{headers: map[string][]sigItem{"set-cookie": {h(`exp_last_visit`)}}}},
	// `/content/dam/` is AEM's file store, but other sites link to it too, while
	// client libraries under `/etc.clientlibs/` are served only by AEM itself.
	// Edge Delivery Services (formerly Franklin) is another way of serving the
	// same AEM: a page built from a document and two scripts of its own instead of
	// clientlibs
	{"Adobe Experience Manager", signature{body: []sigItem{bw(`/etc\.clientlibs/`, 2), b(`/content/dam/`), b(`/scripts/(?:aem|lib-franklin)\.js`)}}},
	{"Salesforce Commerce Cloud", signature{
		body:    []sigItem{bw(`/on/demandware\.static/`, 2), b(`/on/demandware\.store/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bdwsid=`), h(`\bdwanonymous_`)}},
	}},
	{"Framer", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Framer`), b(`framerusercontent\.com`), b(`\bdata-framer-`)},
		headers: map[string][]sigItem{"server": {h(`^Framer\b`)}},
	}},
	{"Weblium", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Weblium`), bw(`res2\.weblium\.site`, 2)}}},
	{"Discourse", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Discourse`), bw(`\bdata-discourse-setup`, 2), b(`discourse-cdn\.com`)}}},
	{"MediaWiki", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+MediaWiki`), b(`/load\.php\?lang=`), b(`\bRLCONF`)}}},
	{"Moodle", signature{
		body:    []sigItem{b(`/theme/yui_combo\.php`), b(`/theme/image\.php`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bMoodleSession`)}},
	}},
	{"Plone", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Plone`), b(`\+\+plone\+\+`), b(`\+\+resource\+\+`)}}},
	{"PHPShop", signature{
		body:    []sigItem{bw(`/phpshop/templates/`, 2)},
		headers: map[string][]sigItem{"x-powered-by": {h(`PHPShop`)}},
	}},
	{"PlatformaLP", signature{body: []sigItem{bw(`\bdata-plp-analytics`, 2), b(`/widgets/plp-analytics`), b(`\bplp-field`)}}},
	{"ReadyScript", signature{body: []sigItem{b(`/storage/system/resized/`), b(`/templates/[^/"]+/resource/`), b(`/cache/resource/min_`)}}},
	{"InstantCMS", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+InstantCMS`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`InstantCMS`)}, "set-cookie": {h(`\bInstantCMS\[`)}},
	}},
	{"StoreLand", signature{
		body:    []sigItem{b(`\.storeland\.ru/`)},
		headers: map[string][]sigItem{"x-generator": {h(`StoreLand`)}},
	}},
	{"Mobirise", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Mobirise`), bw(`assets/mobirise/`, 2)}}},
	{"GoDaddy Website Builder", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Go Daddy Website Builder`), bw(`img1\.wsimg\.com/isteam/`, 2)}}},
	{"Hostinger Website Builder", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Hostinger`), bw(`\.zyrosite\.com/`, 2)},
		headers: map[string][]sigItem{"x-powered-by": {h(`HostingerWebsiteBuilder`)}},
	}},
	// Google's ESF server header is shared by many services; only the resources of
	// Google Sites itself are a feature
	{"Google Sites", signature{body: []sigItem{bw(`gstatic\.com/atari/`, 2)}}},
	{"Readymag", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Readymag`), bw(`\.rmcdn\.net/`, 2)}}},
	// Cargo 2 serves files from cargocollective.com, Cargo 3 from cargo.site and
	// names itself in headers
	{"Cargo", signature{
		body:    []sigItem{bw(`\.cargocollective\.com/`, 2), b(`freight\.cargo\.site`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bCargo\b`)}},
	}},
	// Shopware 6 builds the theme into `/theme/<md5>/`, Shopware 5 keeps it in
	// `/themes/Frontend/`
	{"Shopware", signature{
		body:    []sigItem{bw(`/theme/[0-9a-f]{32}/(css|js)/`, 2), b(`window\.router\['frontend\.`), bw(`/themes/Frontend/`, 2)},
		headers: map[string][]sigItem{"sw-language-id": {h(`.+`)}},
	}},
	{"nopCommerce", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+nopCommerce`), b(`/Themes/[^/]+/Content/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\.Nop\.`)}},
	}},
	{"VTEX", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+vtex\.render-server`), bw(`vteximg\.com\.br`, 2), bw(`vtexassets\.com`, 2)},
		headers: map[string][]sigItem{"x-vtex-cache-status": {h(`.+`)}},
	}},
	{"Shoptet", signature{body: []sigItem{bw(`cdn\.myshoptet\.com`, 2), b(`\bshoptet\.config`)}}},
	{"Shoper", signature{
		body:    []sigItem{bw(`/environment/cache/images/`, 2)},
		headers: map[string][]sigItem{"x-powered-by": {h(`DCSaaS`)}},
	}},
	{"Cafe24", signature{body: []sigItem{bw(`echosting\.cafe24\.com`, 2), b(`/ind-script/`), b(`\bEC_FRONT`)}}},
	// `/web/assets/` without a leading quote matches Mobirise's
	// `assets/web/assets/`
	{"Odoo", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Odoo`), b(`="/web/assets/`), b(`\bdata-oe-model`)}}},
	{"Sylius", signature{body: []sigItem{bw(`id="sylius-`, 2), b(`/bundles/sylius`)}}},
	{"OXID eShop", signature{
		body:    []sigItem{b(`/out/[^/"]+/src/(js|css|img)/`), b(`<!-- OXID eShop`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bsid_key=`)}},
	}},
	{"Gambio", signature{body: []sigItem{bw(`\bdata-gambio-(namespace|controller|widget)`, 2), b(`GXModules/`)}}},
	{"JTL-Shop", signature{
		body:    []sigItem{bw(`\bjtl_token`, 2), b(`templates/NOVA/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bJTLSHOP=`)}},
	}},
	{"Big Cartel", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Big Cartel`), bw(`assets\.bigcartel\.com`, 2)}}},
	// The platform's former name, 3dcart, remains in store markup
	{"Shift4Shop", signature{body: []sigItem{bw(`<!--\s*START: 3dcart`, 2), b(`/assets/templates/common-html5/`), b(`[?&]vcart=[0-9]`)}}},
	{"Simple Machines Forum", signature{body: []sigItem{bw(`\bsmf_(scripturl|theme_url)`, 2), b(`/Themes/default/scripts/`)}}},
	{"NodeBB", signature{
		body:    []sigItem{bw(`/assets/nodebb\.min\.js`, 2)},
		headers: map[string][]sigItem{"x-powered-by": {h(`NodeBB`)}},
	}},
	{"Flarum", signature{
		body:    []sigItem{bw(`\bid="flarum-loading`, 2)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bflarum_session=`)}, "x-powered-by": {h(`Flarum`)}},
	}},
	{"DokuWiki", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+DokuWiki`), b(`/lib/exe/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bDokuWiki=`)}},
	}},
	// DNN runs on ASP.NET, which scores up to 7 (`__VIEWSTATE` and two headers):
	// DNN's own feature weighs the same and wins because it is listed higher
	{"DNN", signature{
		body:    []sigItem{bw(`/DesktopModules/`, 7), bw(`/Portals/[0-9_]+/`, 2), bw(`<meta[^>]+generator[^>]+DotNetNuke`, 7)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bdnn_IsMobile=`)}},
	}},
	{"Grav", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+GravCMS`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bgrav-site-`)}},
	}},
	// Statamic and BookStack run on Laravel and send its cookies, worth up to 4,
	// so their own feature weighs more; otherwise the answer would be Laravel
	{"Statamic", signature{headers: map[string][]sigItem{"x-powered-by": {hw(`Statamic`, 5)}}}},
	{"BookStack", signature{headers: map[string][]sigItem{"set-cookie": {hw(`\bbookstack_session=`, 5)}}}},
	{"ProcessWire", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+ProcessWire`), b(`/site/(templates|assets)/`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`ProcessWire`)}, "set-cookie": {h(`\bwires=`)}},
	}},
	// The `_resources/` path counts only from the root: Albireo CMS keeps it
	// inside `/templates/` (max-3000.com)
	{"Silverstripe", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Silverstripe`), b(`="(https?://[^/"]+)?/_resources/`)}}},
	{"Storyblok", signature{body: []sigItem{b(`a\.storyblok\.com`)}}},
	{"DatoCMS", signature{body: []sigItem{b(`datocms-assets\.com`)}}},
	{"LiveStreet CMS", signature{body: []sigItem{bw(`\bLIVESTREET_SECURITY_KEY`, 2), b(`/templates/skin/`)}}},
	{"Bolt CMS", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Bolt`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bBolt\b`)}},
	}},
	// Bitrix24 sites load the same `/bitrix/js/landing/` as on-premise 1C-Bitrix;
	// only the cloud header tells them apart
	{"Bitrix24.Sites", signature{headers: map[string][]sigItem{"server": {hw(`Bitrix24\.Sites`, 5)}, "x-powered-cms": {h(`Bitrix24\.Sites`)}}}},
	// Carrd writes the page script itself; the "Made with Carrd" badge exists only
	// on free sites
	{"Carrd", signature{body: []sigItem{bw(`var on = addEventListener,\s*off = removeEventListener`, 3), b(`\bicc-credits`)}}},
	{"ClickFunnels", signature{body: []sigItem{bw(`statics\.myclickfunnels\.com`, 2), bw(`id="cf-head-scripts"`, 2)}}},
	{"SITE123", signature{body: []sigItem{bw(`cdn-cms\.f-static\.com`, 2), b(`\bs123-cdn-network`)}}},
	{"Systeme.io", signature{body: []sigItem{bw(`<!-- Created with https://systeme\.io`, 3), b(`d1yei2z3i6k35z\.cloudfront\.net`)}}},
	// Showit draws the pages, but the blog on its sites usually runs on WordPress,
	// which puts up to five of its features into the markup
	{"Showit", signature{body: []sigItem{bw(`static\.showit\.co`, 6)}}},
	{"Bubble", signature{body: []sigItem{bw(`\b_bubble_page_load_data`, 2), b(`\bbubble_session_uid`)}}},
	// The `/media/pages/` path itself is generic; Kirby gives itself away by its
	// fingerprint: a ten-character hash and a timestamp before the file name
	{"Kirby", signature{body: []sigItem{bw(`/media/(pages|site)/[^"'\s]*[0-9a-f]{10}-[0-9]{10}/`, 3)}}},
	{"Pimcore", signature{headers: map[string][]sigItem{"x-powered-by": {h(`\bpimcore\b`)}}}},
	{"Docusaurus", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Docusaurus`)}}},
	{"MkDocs", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+mkdocs`)}}},
	{"VitePress", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+VitePress`)}}},
	{"GitBook", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+GitBook`)}}},
	{"Movable Type", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Movable Type`), b(`/mt-static/`)}}},
	// Teachable and Thinkific run on Rails, Salla and XpressEngine 3 on Laravel,
	// and the framework sends its features worth up to 4: the platform feature is
	// heavier
	{"XpressEngine", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+XpressEngine`, 5)}}},
	{"Sulu", signature{headers: map[string][]sigItem{"x-generator": {h(`\bSulu/`)}}}},
	{"Substack", signature{headers: map[string][]sigItem{"x-served-by": {h(`\bSubstack\b`)}}}},
	{"GetCourse", signature{body: []sigItem{bw(`\bfileServiceThumbnailHost`, 2), b(`class="gc-user-(guest|logged)`), b(`fs\.getcourse\.ru/fileservice/`)}}},
	// Invision Community 4 marks up the page with ipsLayout_ classes; version 3
	// loads styles from public/style_css
	{"Invision Community", signature{body: []sigItem{bw(`\bipsLayout_`, 2), bw(`public/style_css/css_[0-9]+/ipb_`, 2)}}},
	{"Teachable", signature{body: []sigItem{bw(`teachablecdn\.com`, 5)}}},
	{"Thinkific", signature{body: []sigItem{bw(`window\.Thinkific\.`, 5), b(`\bthinkific-cdn`), b(`assets\.thinkific\.com`)}}},
	{"Salla", signature{body: []sigItem{bw(`cdn\.salla\.(network|sa)`, 5)}}},
	{"Tumblr", signature{body: []sigItem{bw(`pre_tumblelog\.js`, 2), b(`assets\.tumblr\.com/client/`)}}},
	{"Gnuboard", signature{body: []sigItem{bw(`var g5_url`, 2), bw(`\bg4_path`, 2)}}},
	{"X-Cart", signature{body: []sigItem{bw(`type="text/x-cart-data"`, 2), b(`<meta[^>]+generator[^>]+X-Cart`)}}},
	{"Hexo", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Hexo`)}}},
	{"Eleventy", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Eleventy`)}}},
	{"VuePress", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+VuePress`)}}},
	{"Mintlify", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Mintlify`)}}},
	{"Pelican", signature{body: []sigItem{b(`href="https?://getpelican\.com`)}}},
	{"Sphinx", signature{body: []sigItem{bw(`_static/documentation_options\.js`, 2)}}},
	{"Nicepage", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Nicepage`), bw(`/nicepage\.css`, 2)}}},
	{"Softr", signature{body: []sigItem{bw(`assets\.softr-files\.com`, 2)}}},
	{"SAP Commerce Cloud", signature{body: []sigItem{bw(`/_ui/responsive/`, 2), b(`\bACC\.config`)}}},
	{"BASE", signature{body: []sigItem{bw(`cf-baseassets\.thebase\.in`, 2), b(`\bbaseec-img-mng`)}}},
	{"Builder.io", signature{body: []sigItem{b(`cdn\.builder\.io/api/v1/image/`)}}},
	{"Contentstack", signature{body: []sigItem{b(`images\.contentstack\.(io|com)/v3/assets/`)}}},
	// A link to business.yandex.ru is not a feature: agencies that set up ads put
	// it there too. Recognised by the "Site created in Yandex Business" badge and
	// images from the business directory
	{"Yandex Business", signature{body: []sigItem{bw(`class="Branding_branding__`, 2), b(`avatars\.mds\.yandex\.net/get-(sprav|altay)`)}}},
	{"Discuz!", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Discuz!`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\b[a-z0-9]{4}_[0-9]{4}_saltkey=`)}},
	}},
	{"LiveJournal", signature{body: []sigItem{bw(`l-stat\.livejournal\.net`, 2)}}},
	{"SkeekS CMS", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+SkeekS CMS`)},
		headers: map[string][]sigItem{"x-powered-cms": {h(`SkeekS CMS`)}},
	}},
	// The server of some MaxSite sites sets a `wordpress_no_cache` cookie, and
	// WordPress scores 2
	{"MaxSite CMS", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+MaxSite CMS`, 2), bw(`/application/maxsite/`, 2)}}},
	{"Danneo CMS", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Danneo`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bDanneo\b`)}},
	}},
	{"Twilight CMS", signature{headers: map[string][]sigItem{"x-powered-cms": {h(`Twilight CMS`)}}}},
	{"S-COREpion CMS", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+S-COREpion`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`S-COREpion`)}, "set-cookie": {h(`\bSCOREPION=`)}},
	}},
	{"Aegea", signature{
		body:    []sigItem{b(`\be2-(note|text)\b`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bAegea\b`)}},
	}},
	{"Vigbo", signature{
		body:    []sigItem{b(`vigbo\.com/cms/`), b(`\bvigbo-cms/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\b_gphw_`)}},
	}},
	{"Eleanor CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Eleanor CMS`)}}},
	{"CMS Made Simple", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+CMS Made Simple`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bCMSSESSID`)}},
	}},
	{"e107", signature{
		body:    []sigItem{bw(`\be107_(themes|images|plugins|files)/`, 2)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\be107\b`)}, "set-cookie": {h(`\be107_csrf=`), h(`\bSESSE107COOKIE`)}},
	}},
	{"XOOPS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+XOOPS`)}}},
	// Publii builds a static site that gets uploaded to any hosting: the server
	// header there comes from IIS and ASP.NET, i.e. 2 points to another name
	{"Publii", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Publii`, 3)}}},
	{"Bludit", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Bludit`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bBludit\b`)}},
	}},
	{"Microweber", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Microweber`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bmw_com_session`)}},
	}},
	// Payload sends its own header next to Next.js and pairs with it, like
	// Storyblok and DatoCMS: the site is built by the frontend, and the content
	// lives in Payload
	{"Payload CMS", signature{headers: map[string][]sigItem{"x-powered-by": {h(`\bPayload\b`)}}}},
	// Django CMS runs on Django, which sends its own features: the plugin name is
	// heavier
	{"Django CMS", signature{body: []sigItem{bw(`\bdjangocms[_-]`, 5)}}},
	{"Neos CMS", signature{body: []sigItem{b(`_Resources/Static/Packages/`), b(`/_Resources/Persistent/`), b(`\bneos-contentcollection`)}}},
	{"eZ Publish", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+eZ Publish`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`eZ Publish`)}, "set-cookie": {h(`\beZSESSID=`)}},
	}},
	{"EC-CUBE", signature{headers: map[string][]sigItem{"set-cookie": {h(`\beccube=`), h(`\bECSESSID=`)}}}},
	{"Backdrop CMS", signature{
		body:    []sigItem{b(`<meta[^>]+content="Backdrop CMS`)},
		headers: map[string][]sigItem{"x-generator": {h(`Backdrop CMS`)}},
	}},
	{"Serendipity", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Serendipity`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bs9y_[0-9a-f]`)}},
	}},
	{"Typecho", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Typecho`)}}},
	{"PHPFusion", signature{
		body:    []sigItem{b(`\binfusions/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bfusion[a-zA-Z0-9]*_(visited|lastvisit)=`)}},
	}},
	{"SPIP", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+SPIP`), b(`local/cache-(gd|vigne)`), b(`spip\.php\?`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bspip_lang=`)}},
	}},
	{"DESTOON", signature{headers: map[string][]sigItem{"x-powered-by": {h(`\bDESTOON\b`)}}}},
	{"DedeCMS", signature{body: []sigItem{b(`\bDedeAjax`)}}},
	{"XWiki", signature{body: []sigItem{b(`class="xwiki-`)}}},
	{"Foswiki", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Foswiki`), b(`class="[^"]*foswiki`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bFOSWIKI`)}},
	}},
	{"MoinMoin", signature{body: []sigItem{b(`/moin_static`)}}},
	{"PunBB", signature{body: []sigItem{b(`\bPUNBB\.env`), b(`punbb\.common`), b(`\bid="brd-main"`)}}},
	// FluxBB forked from PunBB 1.2 and carries its markup: a forum on that ancient
	// PunBB branch answers FluxBB. PunBB recognises its own version by the three
	// features above
	{"FluxBB", signature{body: []sigItem{b(`\bid="brdmain"`)}}},
	{"Vanilla", signature{
		body:    []sigItem{b(`\bvanilla/js/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bVanilla=`)}},
	}},
	{"Hatena Blog", signature{body: []sigItem{b(`cdn\.blog\.st-hatena\.com`), b(`hatenablog-parts\.com`)}}},
	{"Tistory", signature{body: []sigItem{b(`tistory[0-9]?\.daumcdn\.net`), b(`daumcdn\.net/tistory_`)}}},
	{"Pixnet", signature{body: []sigItem{b(`\b(static|pimg)\.1px\.tw`), b(`pic\.pimg\.tw`)}}},
	{"ColorMeShop", signature{
		body:    []sigItem{b(`\bimg\.shop-pro\.jp`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bcolorme_`)}},
	}},
	{"Upgates", signature{
		body:    []sigItem{b(`upgates\.com/_cache/`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bUPGATES_`)}},
	}},
	{"GetSimple CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+GetSimple`)}}},
	{"CMSimple", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+CMSimple`)}}},
	{"ImpressPages", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+ImpressPages`)}}},
	{"CubeCart", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+cubecart`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bCCS_[0-9A-F]{8,}`), h(`\bcc_currency=`)}},
	}},
	{"AbanteCart", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+AbanteCart`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bAC_SF_`)}},
	}},
	{"Rhymix", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Rhymix`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\brx_login_status`)}},
	}},
	// The cookie name of a Spree store depends on the application:
	// `_spree_starter_session`, `_<store>_spree_session`
	{"Spree", signature{headers: map[string][]sigItem{"set-cookie": {h(`spree[a-z0-9_]*_session`)}}}},
	{"OpenCms", signature{body: []sigItem{b(`\bopencms\.`), b(`/export/system/`)}}},
	// eZ Platform is the former name of Ibexa DXP (version 2); sites on it answer
	// with the same header
	{"Ibexa DXP", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Ibexa`)},
		headers: map[string][]sigItem{"x-powered-by": {h(`\bIbexa\b`), h(`eZ Platform`)}},
	}},
	{"ikas", signature{headers: map[string][]sigItem{"x-powered-by": {h(`\bikas\b`)}}}},
	{"Brightspot", signature{headers: map[string][]sigItem{"x-powered-by": {h(`\bBrightspot\b`)}}}},
	{"RedCart", signature{headers: map[string][]sigItem{"set-cookie": {h(`\brc2c-`)}}}},
	{"ShopGold", signature{headers: map[string][]sigItem{"set-cookie": {h(`\beGold=`)}}}},
	{"Sazito", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+Sazito`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bsazito_session`)}},
	}},
	{"Dukaan", signature{body: []sigItem{b(`mydukaan\.io`)}}},
	// Haravan copies Shopify down to the `x-shopid` header, and its stores came
	// out as Shopify: the platform's CDN domain weighs more
	{"Haravan", signature{body: []sigItem{bw(`\bhstatic\.net`, 3)}}},
	{"Bsale", signature{
		body:    []sigItem{b(`\bbsale\.cl`)},
		headers: map[string][]sigItem{"set-cookie": {h(`_bsalemarket_session`)}},
	}},
	{"Cotonti", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Cotonti`)}}},
	{"CONTENIDO", signature{
		body:    []sigItem{b(`<meta[^>]+generator[^>]+CONTENIDO`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\b1frontend=`)}},
	}},
	{"Fork CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Fork CMS`)}}},
	{"Batflat", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Batflat`)}}},
	// An offline site builder uploads ready-made files and keeps no server of its
	// own: recognised by its generator and the directory where it puts shared
	// resources
	{"Adobe Muse", signature{body: []sigItem{b(`\bmuseutils\.js`), b(`\bmuseconfig\.js`), b(`window\.Muse\.assets`), b(`jquery\.musepolyfill`)}}},
	{"WebSite X5", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+WebSite X5`), b(`\bx5engine`)}}},
	// The builder's ready-made files sit on any server, and at just-normlicht.com
	// that is ASP.NET with `__VIEWSTATE`: the generator weighs more, so that the
	// answer is the site's engine rather than the platform under it
	{"Zeta Producer", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Zeta Producer`, 7), b(`\bzpnodefaults`), b(`\bzpColumnItem`), b(`\bzpwButton`)}}},
	{"EverWeb", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+EverWeb`), b(`\bew_css/`), b(`\bew_js/`)}}},
	{"RapidWeaver", signature{body: []sigItem{b(`\brw_common/`)}}},
	// SitePad is built on a WordPress fork and serves the same files
	// (`wp-embed.min.js` from its own `/site-inc/js/`, `page-template-default`
	// classes), so its generator weighs more; otherwise the answer becomes
	// WordPress
	{"SitePad", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+SitePad`, 3)}}},
	// The platform's name in `og:site_name` and `twitter:title` is not a feature:
	// Textpattern was declared this way by its own forum and its Jekyll
	// documentation, and WikkaWiki by a WordPress blog under wikkawiki.org
	{"Textpattern CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Textpattern`)}}},
	{"Tiki Wiki CMS Groupware", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Tiki Wiki`), b(`\btiki-index\.php`), b(`\btiki-[a-z_]+_rss\.php`)}}},
	{"TiddlyWiki", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+TiddlyWiki`), b(`\btc-btn-invisible`), b(`\btc-image-button`)}}},
	{"WikkaWiki", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+WikkaWiki`)}}},
	{"YesWiki", signature{body: []sigItem{b(`\byw-topnav`), b(`\byw-main`), b(`\byw-footer`), b(`/tools/bazar/`)}}},
	// `twikiLink` is not a feature, even though the class belongs to TWiki: in
	// TiddlyWiki markup it matches at the end of an escaped `\t\twikiLinks`,
	// because there is a word boundary between a backslash and a letter too
	{"TWiki", signature{body: []sigItem{b(`\btwikiTable`), b(`/twiki/pub/`), b(`/twiki/bin/`)}}},
	{"Apache JSPWiki", signature{body: []sigItem{b(`\bWiki\.jsp`)}}},
	{"Atlassian Confluence", signature{body: []sigItem{b(`\bconfluence-base-url`), b(`\bconfluence-context-path`), b(`\bconfluence-embedded-file-wrapper`), b(`confluence\.fe\.page-load`)}}},
	{"ikiwiki", signature{body: []sigItem{b(`\bikiwiki\.cgi`)}}},
	{"Omeka", signature{body: []sigItem{b(`/application/views/scripts/`), b(`\bOmeka\.(?:skipNav|showAdvancedForm)`)}}},
	{"Elgg", signature{
		body:    []sigItem{b(`\belgg-body`), b(`\belgg-menu-`), b(`\belgg-module`), b(`\b__elgg_token`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bElgg=`)}},
	}},
	{"HumHub", signature{body: []sigItem{b(`\bhumhub-app\.(?:js|css)`)}}},
	// Federated platforms run on frameworks that send their own cookies (Rails for
	// Mastodon, Laravel for Pixelfed), so their own feature weighs more: the
	// answer must be the engine, not the framework under it. A bare
	// `id="mastodon"` is not a feature: that id is used for a link icon to
	// Mastodon in other people's markup (`<symbol id="mastodon">` on
	// thenew.institute, a Kirby site). Mastodon's own app hangs its state on the
	// same node in `data-props` and serves its themes as its own pack
	{"Mastodon", signature{body: []sigItem{bw(`id="mastodon"[^>]*data-props`, 5), bw(`data-props="[^"]*"\s+id="mastodon"`, 5), bw(`/packs/[a-z]+/mastodon`, 2)}}},
	{"Misskey", signature{body: []sigItem{b(`<meta[^>]+application-name[^>]+Misskey`)}}},
	{"Pleroma", signature{body: []sigItem{b(`/api/pleroma/`)}}},
	{"Pixelfed", signature{body: []sigItem{bw(`\bpixelfed-icon-color`, 5)}}},
	{"PeerTube", signature{body: []sigItem{b(`og:platform"\s+content="PeerTube`), b(`content="PeerTube"\s+property="og:platform`)}}},
	{"Mattermost", signature{body: []sigItem{b(`<meta[^>]+application-name[^>]+Mattermost`)}}},
	{"MyBB", signature{body: []sigItem{b(`jscripts/general\.js`), b(`\bmy_post_key`)}}},
	// `index.php?t=` is not a feature, even though it is FUDforum's URL scheme:
	// the same string showed up in a visit counter on swarajyamag.com (Quintype)
	// and on thequint.com. The forum is recognised by its cookie
	{"FUDforum", signature{
		headers: map[string][]sigItem{"set-cookie": {h(`\bfud_session`)}},
	}},
	{"XMB", signature{
		body:    []sigItem{b(`\bctrtablerow`), b(`\brghttablerow`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bxmblv`)}},
	}},
	{"YaBB", signature{body: []sigItem{b(`\bYaBB\.pl`)}}},
	{"Philomena", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Philomena`), b(`content="Philomena`)}}},
	{"Gridsome", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Gridsome`)}}},
	{"Quarto", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+quarto-`)}}},
	{"Nextra", signature{body: []sigItem{b(`\bnextra-`)}}},
	{"Retype", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Retype`)}}},
	{"Rspress", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Rspress`)}}},
	// Cecil and Lume write `<meta content="… " name=generator>`: the attribute
	// name comes after the value and without quotes, so the feature matches the
	// value
	{"Cecil", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Cecil`), b(`content="Cecil [0-9]`)}}},
	// Octopress is a layer over Jekyll and declares both in one generator line:
	// its own feature weighs more; otherwise the answer becomes Jekyll
	{"Octopress", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Octopress`, 2), b(`\boctopress\.js`)}}},
	{"Lume", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+\bLume\b`), b(`content="Lume [0-9a-f]`)}}},
	{"Saber", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Saber`)}}},
	{"Scully", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Scully`)}}},
	{"Bridgetown", signature{body: []sigItem{b(`/_bridgetown/`)}}},
	// The blogging platform is recognised by its CDN host: it stays even after the
	// blog moves to the owner's domain
	{"TypePad", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+typepad`)}}},
	{"JUGEM", signature{body: []sigItem{b(`\bjugemTracker`), b(`\bjugemkey\.jp`)}}},
	{"Postach.io", signature{body: []sigItem{b(`cdn-static\.postach\.io`)}}},
	// Svbtle is a Rails app, and `authenticity_token` weighs 2: its own feature
	// weighs more; otherwise the answer becomes the framework
	{"Svbtle", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Svbtle`, 3), bw(`lightning\.svbtle\.com`, 3)}}},
	{"Superblog", signature{body: []sigItem{b(`\bsuperblogcdn\.com`)}}},
	{"Notion", signature{body: []sigItem{b(`\bnotion-html`), b(`\bnotion-version`)}}},
	{"Super", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+"Super"`)}}},
	{"Canva", signature{body: []sigItem{b(`\b__canva_website_bootstrap__`), b(`\bcanva_scriptExecutor`)}}},
	{"Lovable", signature{body: []sigItem{b(`<meta[^>]+author[^>]+Lovable`)}}},
	{"Umso", signature{body: []sigItem{b(`\bumsousercontent\.com`)}}},
	{"Instapage", signature{body: []sigItem{b(`\bfastcdn\.co`)}}},
	{"Plasmic", signature{body: []sigItem{b(`\bplasmic-`), b(`\b__wab_`)}}},
	{"Voog", signature{body: []sigItem{b(`static\.voog\.com`)}}},
	{"Pixpa", signature{body: []sigItem{b(`\bpixpa-menu`), b(`\bpixpa-marqueesize`)}}},
	{"WebWave", signature{body: []sigItem{b(`\bwebwave\.isDefAndNotNull`)}}},
	// A bare `ycode` cannot be a feature: the fragment sits inside ordinary
	// `currencyCode` and `countryCode`, and sites on Mono.net were named Ycode
	// because of it
	{"Ycode", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Ycode`)}}},
	{"GreatPages", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+GreatPages`)}}},
	{"STUDIO", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Studio\.Design`)}}},
	{"Mono.net", signature{body: []sigItem{b(`\bcdnmns\.com`)}}},
	{"LadiPage", signature{body: []sigItem{b(`\bladicdn\.com`)}}},
	{"Pagecloud", signature{body: []sigItem{b(`app-assets\.pagecloud\.com`)}}},
	{"KlickPages", signature{body: []sigItem{b(`static-public\.kpages\.com\.br`)}}},
	// Oopy and Peraichi serve Notion content but from their own CDN hosts: their
	// pages have no `notion-html` and `notion-version` markers, so there is no
	// conflict with Notion
	{"Oopy", signature{body: []sigItem{bw(`oopy\.lazyrockets\.com`, 2)}}},
	// The platform's name in markup means nothing: at notosiki.co.jp it is a
	// banner advertising it (`top_main_peraichi.png`). The feature is the host of
	// its pages, and it weighs more than Django, which the platform itself is
	// written in
	{"Peraichi", signature{body: []sigItem{bw(`\bperaichiapp\.com`, 3)}}},
	{"CKAN", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+\bckan\b`)}}},
	// Arc XP builds the page with its Fusion engine, and that name appears in the
	// markup of every block
	{"Arc XP", signature{body: []sigItem{b(`\bfusion-app`), b(`/pf/dist/`)}}},
	{"Quintype", signature{body: []sigItem{b(`\bquintype\.io`), b(`\bqlitics\.com`), b(`\bquintype-ace/`)}}},
	{"RebelMouse", signature{body: []sigItem{b(`assets\.rebelmouse\.io`)}}},
	{"WHMCS", signature{body: []sigItem{b(`\bwhmcsBaseUrl`)}}},
	{"WoltLab Suite", signature{
		body:    []sigItem{b(`\bWCF\.[A-Z]`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bw_user_session`)}},
	}},
	{"NamelessMC", signature{body: []sigItem{b(`\bnameless-deps-dist`), b(`\bnamelessmc_`)}}},
	{"Jahia", signature{body: []sigItem{b(`/modules/javascript-modules-engine/`)}}},
	{"Finalsite", signature{body: []sigItem{b(`\bfsElement`), b(`\bfinalsite\.net`)}}},
	{"E-monsite", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+e-monsite`)}}},
	{"JouwWeb", signature{body: []sigItem{b(`assets\.jwwb\.nl`)}}},
	{"Flazio", signature{body: []sigItem{b(`\bflazio\.org`)}}},
	{"MotoCMS", signature{body: []sigItem{b(`/mt-includes/`), b(`/mt-content/`)}}},
	{"Indexhibit", signature{body: []sigItem{b(`\bndxz-studio`), b(`<meta[^>]+generator[^>]+Indexhibit`)}}},
	{"ReadMe", signature{body: []sigItem{b(`cdn\.readme\.io`)}}},
	{"Indico", signature{body: []sigItem{b(`\bIndicoCSPNonce`), b(`\bindico\.modules\.`)}}},
	{"NukeViet", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+NukeViet`)}}},
	// PyroCMS runs on Laravel, which sends its cookies worth up to 4: the platform
	// feature weighs more; otherwise the answer would be the framework
	{"PyroCMS", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+PyroCMS`, 5)}}},
	{"Subrion", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Subrion`)}}},
	// The name Sitefinity in text means nothing, it is on every vendor page about
	// it: the feature is its own namespaces in the markup
	{"Sitefinity", signature{body: []sigItem{b(`\bSitefinity\.(?:Pages|Frontend|Services)`), b(`\bSitefinitySiteMap`)}}},
	{"SiteVision", signature{body: []sigItem{b(`\bSiteVision\.css`), b(`\bsitevision-`)}}},
	{"Botble CMS", signature{body: []sigItem{b(`themes/botble/`)}}},
	{"UNA", signature{body: []sigItem{b(`/inc/js/classes/BxDol`)}}},
	{"Weblication", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Weblication`)}}},
	// Dynamicweb and Elcom run on ASP.NET, and `__VIEWSTATE` with its two headers
	// weighs up to 7: the platform feature weighs no less, a tie goes to the entry
	// listed higher, and ASP.NET sits at the end of the table
	{"Dynamicweb", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Dynamicweb`, 7)}}},
	{"Thelia", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Thelia`)}}},
	{"webEdition", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+webEdition`)}}},
	{"Cloudrexx", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+cloudrexx`)}}},
	{"Elcom", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+elcomCMS`, 7)}}},
	{"Plate", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+\bPlate\b`)}}},
	{"InterRed", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+interred`)}}},
	{"Procurios", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Procurios`)}}},
	// A bare `pobo` cannot be a feature: Czech "pobočky" in Latin letters gives
	// `pobocky/` in a URL, and an unrelated store got the platform because of it
	{"Pobo", signature{body: []sigItem{b(`\bpobo\.space`)}}},
	{"Labrador CMS", signature{body: []sigItem{b(`/_labrador/`)}}},
	{"Chayns", signature{body: []sigItem{b(`chayns-res\.tobit\.com`)}}},
	{"MaxiCMS", signature{body: []sigItem{b(`\bmaxicms\.nl`)}}},
	{"a-blog cms", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+a-blog cms`)}}},
	{"OpenNemas", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+OpenNemas`)}}},
	{"imperia CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+IMPERIA`), b(`\bImperia-Live-Info`), b(`imperia-website/`)}}},
	{"K-Sup", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+K-Sup`)}}},
	{"Jalios", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Jalios`)}}},
	// Ametys writes its generator backwards, like Cecil and Lume: the value comes
	// before the attribute name
	{"Ametys", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Ametys`), b(`content="Ametys v`)}}},
	{"Melis Platform", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Melis Platform`)}}},
	{"Chameleon system", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Chameleon`)}}},
	{"Hocalwire", signature{body: []sigItem{b(`\bhocalwire-`)}}},
	// Kooboo and C1 CMS run on ASP.NET (up to 7), iEXExchanger on Laravel (up to
	// 4): the platform feature weighs no less than its base
	{"Kooboo CMS", signature{body: []sigItem{bw(`/Kooboo-(?:WebResource|Resource|Submit)/`, 7)}}},
	{"C1 CMS", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+C1 CMS`, 7)}}},
	{"iEXExchanger", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+iEXExchanger`, 5), bw(`\biexInitialGuideData`, 5)}}},
	{"Pagekit", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Pagekit`), b(`/packages/pagekit/`)}}},
	{"ERPNext", signature{body: []sigItem{b(`/assets/erpnext/`)}}},
	{"Ensi", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Ensi Platform`)}}},
	{"Oracle Application Express", signature{
		body:    []sigItem{b(`\bwwv_flow\.`), b(`\bapex_img_dir\b`)},
		headers: map[string][]sigItem{"set-cookie": {h(`\bORA_WWV_`)}},
	}},
	{"Powergap", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+POWERGAP`)}}},
	// Omni CMS puts resources under `/_resources/`, like Silverstripe, and
	// publishes to any server, IIS included: the editor login link with the
	// customer's skin must outweigh both
	{"Omni CMS", signature{body: []sigItem{bw(`omniupdate\.com/(?:11/\?skin=|files/content\?)`, 7)}}},
	{"DM Polopoly", signature{body: []sigItem{b(`\bpolopoly_fs(?:/|%2F)`)}}},
	// The CM4all builder is resold by hosting providers under their own brands
	// (Strato in Germany, planeetta.net in Finland), so the entry is named after
	// the engine, not a brand
	{"CM4all", signature{body: []sigItem{b(`/\.cm4all/`), b(`\bcm4all\.widgets`)}}},
	{"HCL Commerce", signature{body: []sigItem{b(`/wcsstore/`)}}},
	// Shoplazza signs its response with an `x-powered-by: ASP.NET` header it does
	// not have, and Shopline and YouCan run on Laravel: the platform feature must
	// outweigh both
	{"Shoplazza", signature{body: []sigItem{bw(`\bshoplazza://`, 5), bw(`\bshoplazza-product-snippet`, 5)}}},
	{"Shopline", signature{body: []sigItem{bw(`\bshoplineimg\.com`, 5)}}},
	{"YouCan", signature{body: []sigItem{bw(`\byoucan\.shop/(?:stores|store-front)/`, 5)}}},
	// IdoSell's `/gfx/<language>/` path is shared with other sites (larena.it on
	// Polopoly); the features are its classes and content directory
	{"IdoSell Shop", signature{body: []sigItem{b(`\biai-(?:recaptcha|shop)`), b(`\biai_cookie`), b(`/data/include/cms/`)}}},
	{"ePages", signature{body: []sigItem{b(`/epages/[^/"'\s]+\.sf\b`), b(`\bepages\.(?:text|image|html|structure|base)\b`)}}},
	{"Ueeshop", signature{body: []sigItem{b(`\bly200-cdn\.com`), b(`\bueeshop_config\b`)}}},
	// A blogger.com link in share buttons and the plugin path `assets/components/`
	// give Blogger and MODX one point each on ShopBase and xt:Commerce stores: the
	// platform feature weighs 2; otherwise a tie goes to the entry listed higher
	{"ShopBase", signature{body: []sigItem{bw(`img\.shopbase\.com/`, 2)}}},
	{"Gomag", signature{body: []sigItem{b(`\bgomagcdn\.ro`)}}},
	{"Unas", signature{body: []sigItem{b(`\.unas\.hu/element/`)}}},
	{"Sky-Shop", signature{body: []sigItem{b(`\bSkyShopModule`), b(`\bskyshop-container`)}}},
	{"Intershop", signature{body: []sigItem{b(`/INTERSHOP/(?:static|web)/WFS/`), b(`\bINTERSHOP\.enfinity`)}}},
	{"xtCommerce", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+xt:Commerce`, 2), b(`/xtFramework/`)}}},
	{"Bizweb", signature{body: []sigItem{b(`\bbizweb\.dktcdn\.net`)}}},
	{"Miva", signature{body: []sigItem{b(`\bmerchant\.mvc\?`), b(`\bmvga_tracker`), b(`\bmivaJS\b`)}}},
	{"Adobe Portfolio", signature{body: []sigItem{b(`\bcdn\.myportfolio\.com/`), b(`\bpro2-bar-s3-cdn-cf\d*\.myportfolio\.com/`)}}},
	{"Bloomreach", signature{body: []sigItem{b(`/binaries/(?:[a-z]+/)?content/gallery/`), b(`\bhippo(?:gallery|std):`), b(`\bhippoecm\.hst\b`)}}},
	{"Stackbit", signature{body: []sigItem{b(`\bdata-sb-(?:object-id|field-path)\b`), b(`\bstackbit_(?:model_type|page_meta)\b`)}}},
	{"Kontent.ai", signature{body: []sigItem{b(`\bkc-usercontent\.com`)}}},
	{"Unicorn Platform", signature{body: []sigItem{b(`\bunicorn-images\.b-cdn\.net`), b(`unicornplatform\.com/static/`)}}},
	{"Turbify", signature{body: []sigItem{b(`\bturbifycdn\.com/(?:ty|aah)/`)}}},
	{"PubLive", signature{body: []sigItem{b(`\bpublive\.(?:online|com)/fit-in/`), b(`\bpublive-(?:slot|logo|dynamic)`)}}},
	// A template exported from Webflow takes `data-wf-page` and `data-wf-site`
	// with it, and on Apostrophe (fanatee.com behind Express) Webflow scored as
	// much: Apostrophe's area markup weighs more
	{"ApostropheCMS", signature{body: []sigItem{bw(`\bapos-(?:area|frontend|refreshable)\b`, 2), bw(`\bapostrophe-widgets\b`, 2)}}},
	// Tridion's content id `tcm:<publication>-<item>-<type>` gets into the markup
	// from a service comment on the page
	{"SDL Tridion", signature{body: []sigItem{b(`\btcm:\d+-\d+-\d+\b`)}}},
	{"Spring for creators", signature{body: []sigItem{b(`\bteespring\.com/v3/image/`), b(`\bteespringId\b`)}}},
	{"ButterCMS", signature{body: []sigItem{b(`\bcdn\.buttercms\.com/`)}}},
	// CrownPeak publishes to IIS, like Omni CMS, and ASP.NET scores up to 7 there
	{"CrownPeak", signature{body: []sigItem{bw(`\bcrownpeak\.searchg2`, 7)}}},
	// Wagtail and Zid run on Django (up to 4), Shoprenter is an OpenCart fork with
	// its paths, SIDEARM publishes on ASP.NET, novomind iSHOP sends Laravel
	// cookies: the platform feature must outweigh its base. For Wagtail it is the
	// name of an image rendition: the filter (`fill-300x200`, `format-webp`) and
	// the key `2e16d0ba`
	{"Wagtail", signature{body: []sigItem{
		bw(`\.(?:fill|width|height|max|min|scale)-\d+(?:x\d+)?(?:-c\d+)?\.format-(?:webp|avif|jpeg|png)\b`, 5),
		bw(`\.2e16d0ba\.(?:fill|width|height|max|min|scale|original)`, 5),
		bw(`/media/images/[^"'\s]+\.(?:fill|width|height|max|min)-\d+`, 5),
	}}},
	{"Zid", signature{body: []sigItem{bw(`\bmedia\.zid\.store/`, 5)}}},
	{"Shoprenter", signature{body: []sigItem{bw(`\bcdn\.shoprenter\.hu/`, 5)}}},
	{"SIDEARM Sports", signature{body: []sigItem{bw(`\bsidearmComponents\b`, 7), bw(`\bsidearm-icon\b`, 7), bw(`\bsidearm\.nextgen\.sites`, 7)}}},
	{"novomind iSHOP", signature{body: []sigItem{bw(`\bishop_javascript_evaluation_result\b`, 5), bw(`/ishop-api/events/`, 5), bw(`\bishop\.api\.management\.`, 5)}}},
	// A Centra storefront is built by a headless CMS whose CDN host weighs 1: for
	// a store visitor the commerce platform matters more
	{"Centra", signature{body: []sigItem{bw(`\bcentracdn\.net/`, 2)}}},
	{"KQS.store", signature{body: []sigItem{b(`\bkqs_off\(\)`), b(`\bkqs-iks\b`)}}},
	{"CCV Shop", signature{body: []sigItem{b(`/Plugins/jQuery/css/website/`)}}},
	{"MakeShop", signature{body: []sigItem{b(`\bmakeshop(?:-multi-images)?\.akamaized\.net/`), b(`\bMakeShopChildCategory\b`)}}},
	// The Ecwid widget is embedded in other sites, and there the answer stays
	// their engine: the entry sits below the engines and loses ties to them
	{"Ecwid", signature{body: []sigItem{b(`\becwid_(?:html|body)\b`), b(`app\.ecwid\.com/script\.js`)}}},
	// The `shared-assets/components/` path on THG's CDN gives MODX one point
	{"THG Ingenuity", signature{body: []sigItem{bw(`\bstatic\.thcdn\.com/`, 2)}}},
	// EasyStore runs on Laravel, LocomotiveCMS and Sharetribe on Rails, Cart.com,
	// Smartstore and Mura CMS send ASP.NET headers: the feature weighs more than
	// the base
	{"EasyStore", signature{body: []sigItem{bw(`\bEasyStore\.Event\.dispatch\(`, 5), bw(`\beasystore\.co/\d+/themes/`, 5)}}},
	{"LocomotiveCMS", signature{body: []sigItem{bw(`/assets/locomotive/`, 5)}}},
	{"Sharetribe", signature{body: []sigItem{bw(`\bsharetribe\.com/images/`, 5), bw(`\bsharetribe_promo`, 5)}}},
	{"Cart.com", signature{body: []sigItem{bw(`\bamericommerce\.com/`, 7), bw(`/store/(?:AddToCart|Search|login)\.aspx`, 7), bw(`\bAmeriCommerce-powered-by`, 7)}}},
	{"Smartstore", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Smartstore`, 7), bw(`\bSmartstore\.cmp\.`, 7)}}},
	{"Mura CMS", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Mura CMS`, 7)}}},
	{"Homestead", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Homestead`)}}},
	{"Sellingo", signature{body: []sigItem{b(`\bsellingo/cookieconsent/`), b(`\bsellingo-footer-\d`)}}},
	{"HCL Digital Experience", signature{body: []sigItem{b(`/wps/(?:portal|contenthandler|wcm)\b`)}}},
	{"Koken", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Koken`), b(`\bkoken-internal\b`)}}},
	// Sites migrated from WordPress keep image links into `/wp-content/uploads`:
	// the builder weighs more than that trace
	{"Phoenix Site", signature{body: []sigItem{bw(`\bphoenixsite\.nl/pageomatic/`, 2)}}},
	{"SummerCart", signature{body: []sigItem{b(`\bsummercart\.(?:banners|dynamic_page|rss)\b`)}}},
	{"SoteShop", signature{body: []sigItem{b(`/images/frontend/theme/`)}}},
	{"PinnacleCart", signature{body: []sigItem{b(`\bcontent/cache/skins/`)}}},
	{"Storeden", signature{body: []sigItem{b(`\bstoreden\.(?:net|com)/themes/`), b(`\bstoredenrequestdigitalinvoice`)}}},
	{"ATSHOP", signature{body: []sigItem{b(`\batshop\.io/bundle/`)}}},
	{"Mixin", signature{body: []sigItem{b(`\bmixin_hash_id\b`), b(`\bmixin-get-params-compatibility\b`)}}},
	{"SpotHopper", signature{body: []sigItem{b(`/spothopper/image/fetch/`)}}},
	{"Cosmic", signature{body: []sigItem{b(`\b(?:cdn|imgix)\.cosmicjs\.com/`)}}},
	{"Funnelish", signature{body: []sigItem{b(`\bfunnelish\.com/\d+/\d+/`)}}},
	// Contensis, BSmart, GrandNode and Menufy send ASP.NET headers, Podia runs on
	// Rails: the platform feature weighs more than the base. Contensis also
	// outweighs AEM: nottingham.ac.uk renders the page with Contensis and takes
	// the header from AEM
	{"Contensis", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Contensis`, 7)}}},
	{"BSmart", signature{body: []sigItem{bw(`\bbsmart_style=`, 7), bw(`/bsmart/bsmartstyles/`, 7)}}},
	{"GrandNode", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+grandnode`, 7)}}},
	{"Menufy Website", signature{body: []sigItem{bw(`\bmenufy-footer\b`, 7)}}},
	{"Podia", signature{body: []sigItem{bw(`\b(?:cdn|content)\.podia\.com/`, 5)}}},
	{"Halo", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Halo \d`, 5), bw(`content="Halo \d[^"]*" name="?generator`, 5)}}},
	// A Spryker store takes page content from Bloomreach or Neos, whose traces
	// weigh 1: for a store visitor the commerce platform matters more
	{"Spryker", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+spryker`, 2)}}},
	{"4Partners CMS", signature{body: []sigItem{b(`/atlas/build-new/`)}}},
	{"Convertri", signature{body: []sigItem{b(`\bconvertri\.imgix\.net/`), b(`\bconvertri-script-consent\b`)}}},
	{"Emergent", signature{body: []sigItem{b(`\bemergent\.sh/scripts/emergent-main`)}}},
	{"Future Shop", signature{body: []sigItem{b(`\br2\.future-shop\.jp/fs\.`)}}},
	{"Jumpseller", signature{body: []sigItem{b(`\b(?:cdnx|assets|images)\.jumpseller\.com/`)}}},
	{"LogiCommerce", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+LogiCommerce`), b(`\blogicommerce\.cloud/`)}}},
	{"Prepr", signature{body: []sigItem{b(`\.(?:cdn|stream)\.prepr\.io/`)}}},
	// The Quick.Cart license forbids removing the vendor link, and the template
	// warns about it in a comment
	{"Quick.Cart", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Quick\.Cart`), b(`HIDE LINK "Shopping cart by Quick\.Cart"`)}}},
	{"Quickbutik", signature{body: []sigItem{b(`\b(?:cdn|storage)\.quickbutik\.com/`)}}},
	{"Sana Commerce", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Sana Commerce`), b(`\bsana\.cloud/`)}}},
	{"Scrivito", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Scrivito`), b(`\bscrivito-prerendering-obj-id=`)}}},
	{"TomatoCart", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+TomatoCart`)}}},
	{"WebZi", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Webzi\.ir`), b(`\bwebzi\.ir/static/`)}}},
	{"microCMS", signature{body: []sigItem{b(`\bmicrocms-assets\.io/`)}}},
	// AIMEOS and Shuttle run on Laravel, Akinon on Django, HighStore, Milestone,
	// SellersCommerce and Ticimax send ASP.NET headers: the feature weighs no less
	// than the base
	{"AIMEOS", signature{body: []sigItem{bw(`<meta[^>]+application-name[^>]+Aimeos`, 5), bw(`/aimeos\.css\b`, 5)}}},
	{"Shuttle", signature{body: []sigItem{bw(`\bshuttle-storage\.s3\.amazonaws\.com/`, 5)}}},
	{"Akinon", signature{body: []sigItem{bw(`\bakinoncloud\.com/`, 5), bw(`\bakinoncdn\.com`, 5)}}},
	{"HighStore", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+HighStore`, 7)}}},
	{"Milestone CMS", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Milestone CMS`, 7)}}},
	{"SellersCommerce", signature{body: []sigItem{bw(`\bsellerscommerce\.com/scassets/`, 7)}}},
	{"Ticimax", signature{body: []sigItem{bw(`\bticimax\.cloud/`, 7)}}},
	// Frontastic serves a storefront on top of Contentful, whose CDN host weighs 1
	{"Frontastic", signature{body: []sigItem{bw(`\bfrontastic_canonical_route\b`, 2), bw(`\bfrontastic\.frontend\.`, 2)}}},
	// The generator `Hostinger AI Builder` also matches the Hostinger Website
	// Builder feature, which is listed higher
	{"Hostinger Horizons", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Hostinger AI Builder`, 2)}}},
	{"Adwos CMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Adwos CMS`)}}},
	{"AlvandCMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+AlvandCMS`)}}},
	{"AsciiDoc", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+AsciiDoc`)}}},
	{"Brownie", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Brownie`), b(`/brownie/scripts/`)}}},
	{"Caisy", signature{body: []sigItem{b(`\bassets\.caisy\.io/`)}}},
	{"CloudSuite", signature{body: []sigItem{b(`\bs3-cdn\.cloudsuite\.com/`)}}},
	{"CoreMedia", signature{body: []sigItem{b(`<meta name="coremedia:content-id"`)}}},
	{"Easy Orders", signature{body: []sigItem{b(`\beasyorders\.shop/_next/`)}}},
	{"FatherShops", signature{body: []sigItem{b(`\bstatic\.myfathershops\.com/`)}}},
	{"Fynd Platform", signature{body: []sigItem{b(`\bcdn\.fynd\.com/v2/`)}}},
	{"Griddo", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Griddo`)}}},
	{"Kreatio", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Kreatio`), b(`\bkreatio\.net/`)}}},
	{"MyOnlineStore", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Mijnwebwinkel`), b(`\bmyonlinestore\.eu/`)}}},
	{"Mysitefy", signature{body: []sigItem{b(`\bmysitefy\.com/img/`)}}},
	{"OnUniverse", signature{body: []sigItem{b(`\bonuniverse-assets\.imgix\.net/`)}}},
	{"Shopcada", signature{body: []sigItem{b(`\bshopcada-grid-`)}}},
	{"Simplo7", signature{body: []sigItem{b(`\bsimplo7\.net/static/`)}}},
	{"SmartWeb", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+SmartWeb`), b(`\bsmartweb-static\.com/`)}}},
	{"Swell", signature{body: []sigItem{b(`\bcdn\.swell\.store/`)}}},
	{"The Church Co", signature{body: []sigItem{b(`\bthechurchcoassets\.com/`), b(`\bthechurchco-production\.`)}}},
	{"Vendre", signature{body: []sigItem{b(`\bvendreCartUpdate\b`)}}},
	{"WebsPlanet", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Websplanet`)}}},
	{"Wikinggruppen", signature{body: []sigItem{b(`<!-- WIKINGGRUPPEN \d`)}}},
	{"Xanario", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+xanario`)}}},
	{"Zoho Commerce", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Zoho Commerce`), b(`\bzohoecommerce\.com/`)}}},
	{"eSyndiCat", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+eSyndiCat`)}}},
	{"phpSQLiteCMS", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+phpSQLiteCMS`)}}},
	{"Congressus", signature{body: []sigItem{b(`\bcongressus-static-backend\.s3\.amazonaws\.com/`)}}},
	{"Dr. Leonardo", signature{body: []sigItem{b(`\bdrleonardo\.com\.(?:templates|dev\.misc)/`)}}},
	// Lede serves a Next.js edition on top of its own WordPress, which scores one
	// point from images at `lede-admin.<domain>/wp-content/`
	{"Lede", signature{body: []sigItem{bw(`\blede_byline_`, 2), bw(`\blede-admin\.`, 2)}}},
	// Bag, Blutui, InnoShop, Sociavore, Twsaa and simploCMS run on Laravel, Nimbu
	// and Shopmaker on Rails, Podpage on Django, ForoshGostar on nopCommerce, Webx
	// sends ASP.NET headers, and Storearmy October's paths: the feature weighs no
	// less than the base. Packman stores score Framer one point; Packman weighs 2
	{"Abicart", signature{body: []sigItem{b(`\bcdn\.abicart\.com/`)}}},
	{"AboutMyClinic", signature{body: []sigItem{b(`\bstatic\.aboutmyclinic\.com/`)}}},
	{"Bag", signature{body: []sigItem{bw(`\bcdn\.shantaweb\.com/`, 5)}}},
	{"Bigshop", signature{body: []sigItem{b(`\bassetspublic\.bigshop\.com\.br/`), b(`\bstatic\.bigshop\.com\.br/`)}}},
	{"Blutui", signature{body: []sigItem{bw(`\bcdn\.blutui\.com/`, 5)}}},
	{"Comarch e-Sklep", signature{body: []sigItem{b(`\bstatic\.comarchesklep\.pl/`)}}},
	{"Crystallize", signature{body: []sigItem{b(`\bmedia\.crystallize\.com/`)}}},
	{"Dukany", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Dukany`), b(`\borderapi\.dukany\.io`)}}},
	{"Ferret One", signature{body: []sigItem{b(`\bferret-one\.akamaized\.net/`)}}},
	{"Florist Touch", signature{body: []sigItem{b(`\bclientassets\.floristtouch\.co\.uk/`), b(`\bstatic\.floristtouch\.com/`)}}},
	{"ForoshGostar", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+foroshGostar`, 5)}}},
	{"InnoShop", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+InnoShop`, 5)}}},
	{"Jibres", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Jibres`)}}},
	{"Kartmax", signature{body: []sigItem{b(`\bpictures\.kartmax\.in/`)}}},
	{"LiveBooks", signature{body: []sigItem{b(`\bstatic\.livebooks\.com/`)}}},
	{"LiveSite", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+liveSite`)}}},
	{"LogaTech", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+LogaTech`)}}},
	{"Mattaki", signature{body: []sigItem{b(`\bcdn\.mattaki\.com/`)}}},
	{"MGPanel", signature{body: []sigItem{b(`\bmgpanel-v\d+\.s3\.[a-z0-9.-]*amazonaws\.com/`)}}},
	{"NagaCommerce", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+NagaCommerce`), b(`\bassets\.nagacommerce\.com/`)}}},
	{"Nimbu", signature{body: []sigItem{bw(`\b(?:cdn|static)\.nimbu\.io/`, 5)}}},
	{"Orbit Commerce", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Orbit Commerce`)}}},
	{"Packman", signature{body: []sigItem{bw(`\bcdn\.packman\.(?:ai|app)/`, 2)}}},
	{"Pixelesq", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Pixelesq`)}}},
	{"Plugo", signature{body: []sigItem{b(`\bapi\.plugo\.world/v1/shop/`)}}},
	{"Podpage", signature{body: []sigItem{bw(`\b(?:img|static)\.podpage\.com/`, 5)}}},
	{"Portal", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Portal Site Builder`), b(`\bstatic\.portal\.ir/`)}}},
	{"Poski", signature{body: []sigItem{b(`\bcdn\.poski\.com/`)}}},
	{"Publishrr", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Publishrr`)}}},
	{"Qukasoft", signature{body: []sigItem{b(`\bcdn\.qukasoft\.com/`)}}},
	{"Scayle", signature{body: []sigItem{b(`\.checkout\.api\.scayle\.cloud/`)}}},
	{"Selless", signature{body: []sigItem{b(`\bcdn\.selless\.us/`)}}},
	{"ShopFactory", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+ShopFactory`)}}},
	{"Shopblocks", signature{body: []sigItem{b(`\bstatic\.shopblocks\.com/`)}}},
	{"Shopmaker", signature{body: []sigItem{bw(`\bstatic\.shopmaker\.com/`, 5)}}},
	{"ShoutCMS", signature{body: []sigItem{b(`\bassets-web\d+\.shoutcms\.net/`)}}},
	{"Silex", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Silex v\d`)}}},
	{"Sixshop", signature{body: []sigItem{b(`\bstatic\.sixshop\.com/`)}}},
	{"Sketchanet", signature{body: []sigItem{b(`\bcors\.sketchanet\.com/`)}}},
	{"Sociavore", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Sociavore`, 5)}}},
	{"Solusquare OmniCommerce Cloud", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Solusquare`)}}},
	{"Storearmy", signature{body: []sigItem{bw(`\bcdn\.storearmy\.com/`, 5)}}},
	{"Storeino", signature{body: []sigItem{b(`\bstoreno\.b-cdn\.net/`)}}},
	{"TakeDrop", signature{body: []sigItem{b(`\bmain\.takedropstorage\.com/`)}}},
	{"Tokeet", signature{body: []sigItem{b(`\bcdn\.tokeet\.com/`)}}},
	{"Twsaa", signature{body: []sigItem{bw(`\bcdn\.twsaa\.com/`, 5)}}},
	{"VerseOne", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+VerseOne`)}}},
	{"Webready", signature{body: []sigItem{b(`\bcdn\.usewebready\.com/`)}}},
	{"Webx", signature{body: []sigItem{bw(`\bstatic\d*\.webx\.pk/`, 7)}}},
	{"WiziShop", signature{body: []sigItem{b(`\bsentry\.wizishop\.com/`)}}},
	{"Yclas", signature{body: []sigItem{b(`\byclas\.nyc3\.cdn\.digitaloceanspaces\.com/`)}}},
	{"iWiki", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+iWink CMS`)}}},
	{"immediaCMS", signature{body: []sigItem{b(`\bimmediac\.blob\.core\.windows\.net/`)}}},
	{"simploCMS", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+simploCMS`, 5)}}},
	{"Behzi", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Behzi`)}}},
	{"Bricksite", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Bricksite`)}}},
	// Saffire sends ASP.NET headers, CommentSold runs on Laravel. Single traces of
	// WordPress and OpenCart occur on Assemble and Brushd sites
	{"Alboom Prosite", signature{body: []sigItem{b(`\bbifrost\.alboompro\.com/static`), b(`\bcdn-cp\.alboompro\.com/`)}}},
	{"Brushd", signature{body: []sigItem{bw(`\bassets\.brushd\.co/`, 2)}}},
	{"Maglr", signature{body: []sigItem{b(`\bsystem\.maglr\.com/`), b(`\bdata\.maglr\.com/`)}}},
	{"Saffire", signature{body: []sigItem{bw(`\bcdn\.saffire\.com/theme-files`, 7)}}},
	{"Assemble", signature{body: []sigItem{bw(`\bcdn\.assemble\.me/`, 2)}}},
	{"Cococart", signature{body: []sigItem{b(`\bcdn\.cococart\.co/`)}}},
	{"CommentSold", signature{body: []sigItem{bw(`\b(?:s3|cdn)\.commentsold\.com/`, 5)}}},
	{"Control", signature{body: []sigItem{b(`\bcdn\.cntrl\.site/`), b(`<meta[^>]+generator[^>]+cntrl\.site`)}}},
	{"Webzie", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+Webzie`)}}},
	// Mabisy, OrderPort and Sqwiz send ASP.NET headers, Zepio and Sharing run on
	// Laravel
	{"Contentder", signature{body: []sigItem{b(`\bcdn\.contentder\.com/`)}}},
	{"Inblog", signature{body: []sigItem{b(`\bimage\.inblog\.dev/`), b(`<meta[^>]+generator[^>]+inblog`)}}},
	{"Justo", signature{body: []sigItem{b(`\bwebcdn\.getjusto\.com/`), b(`\btofuu\.getjusto\.com/`)}}},
	{"Mabisy", signature{body: []sigItem{bw(`<meta[^>]+generator[^>]+Mabisy`, 7)}}},
	{"OrderPort", signature{body: []sigItem{bw(`\borderport-webstore\.b-cdn\.net/`, 7)}}},
	{"QUV", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+QUV Homepage Builder`), b(`\bcdn\.quv\.kr/`)}}},
	{"Qshop", signature{body: []sigItem{b(`\bcdn\.qshop\.ai/`), b(`\bqlog-sdk\.qshop\.ai/`)}}},
	{"ShopGate", signature{body: []sigItem{b(`\bdata\.shopgate\.com/`), b(`\bimg-cdn\.shopgate\.com/`)}}},
	{"Shoprocket", signature{body: []sigItem{b(`\b(?:cdn|img)\.shoprocket\.io/`)}}},
	{"StackerHQ", signature{body: []sigItem{b(`\bcdn\.stackerhq\.com/`)}}},
	{"Weezbe", signature{body: []sigItem{b(`\bstatic\.weezbe\.com/`), b(`\bmedias\.weezbe\.com/`)}}},
	{"Zepio", signature{body: []sigItem{bw(`\bcdn\.zepio\.io/`, 5)}}},
	{"eDokan", signature{body: []sigItem{b(`\bstatic\.edokan\.co/`), b(`\bcdn\.edokan\.co/`)}}},
	{"Sharing", signature{body: []sigItem{bw(`generator"\s+content="Sharing"`, 5)}}},
	{"Siter", signature{body: []sigItem{b(`\b(?:cdn|api)\.siter\.io/assets/`)}}},
	{"Sqwiz", signature{body: []sigItem{bw(`/userfiles/sqwiz_`, 7)}}},
	{"BBT bCube NX", signature{body: []sigItem{b(`<meta[^>]+generator[^>]+BBT bCube`)}}},
	{"Plentymarkets", signature{body: []sigItem{bw(`cdn[0-9]+\.plenty(markets|one)\.com/`, 2), b(`<meta[^>]+generator[^>]+plentymarkets`), b(`\bplenty-shop-cookie`)}}},
	{"React", signature{body: []sigItem{bw(`\bdata-reactroot`, 2), bw(`id="root"`, 1)}}},
	// Vue's scoped marker is `data-v-` followed by eight hex digits. Without them
	// the feature matches someone else's attribute: `data-v-align="center"` on
	// RapidWeaver
	{"Vue", signature{body: []sigItem{b(`\bdata-v-[0-9a-f]{8}`)}}},
	{"Angular", signature{body: []sigItem{b(`\bng-app`), b(`\bng-version`)}}},
	// Only the field name `authenticity_token` is specific to Rails. The
	// `csrf-param` and `csrf-token` pair is written by Yii and hand-made backends
	// too: www.rbc.ru has `csrf_token` there
	{"Ruby on Rails", signature{body: []sigItem{bw(`content="authenticity_token"`, 2), bw(`name="authenticity_token"`, 2)}}},
	{"ASP.NET", signature{
		body:    []sigItem{bw(`\b__VIEWSTATE`, 3)},
		headers: map[string][]sigItem{"x-aspnet-version": {h(`.+`)}, "x-powered-by": {h(`ASP\.NET`)}},
	}},
	{"Vite", signature{
		body: []sigItem{b(`type="module"\s+src="/@vite`), b(`type="module"\s+src="/src/`)},
	}},
}

// frontendLayer lists presentation-layer CMSs and frameworks: a site may run
// them on top of any backend (headless), so they do not compete with the
// backend for its single place in the result of SelectWinners.
var frontendLayer = map[string]bool{
	"Next.js": true, "Nuxt.js": true, "Astro": true, "Svelte": true, "Gatsby": true,
	"React": true, "Vue": true, "Angular": true, "Vite": true,
}

// CMSScore is the score of one CMS or framework, in detection order. The order
// decides ties (SelectWinners sorts stably), which is why this is a slice, not
// a map.
type CMSScore struct {
	Name  string
	Score float64
}

// minWinnerScore rejects an answer based on one weak feature: the theme path
// `/themes/<theme>/assets/` weighs 0.5 for October and Winter and appears on
// hundreds of unrelated sites, so without a threshold any site with no other
// features became October
const minWinnerScore = 1

/*
SelectWinners applies the "one site, one engine" rule: among competing backends
only one wins (the highest score; on a tie, the first in detection order), and a
frontend layer is added as a separate entry on top of it (a headless pair such
as Next.js + WordPress). A candidate scoring below minWinnerScore never wins.
*/
func SelectWinners(scores []CMSScore) []string {
	if len(scores) == 0 {
		return nil
	}

	ranked := append([]CMSScore(nil), scores...)
	sortByScoreDesc(ranked)

	var backend, frontend []CMSScore
	for _, s := range ranked {
		if s.Score < minWinnerScore {
			continue
		}
		if frontendLayer[s.Name] {
			frontend = append(frontend, s)
		} else {
			backend = append(backend, s)
		}
	}

	var result []CMSScore
	if len(backend) > 0 {
		result = append(result, backend[0])
	}
	if len(frontend) > 0 {
		result = append(result, frontend[0])
	}
	sortByScoreDesc(result)

	names := make([]string, len(result))
	for i, s := range result {
		names[i] = s.Name
	}
	return names
}

func sortByScoreDesc(scores []CMSScore) {
	sort.SliceStable(scores, func(i, j int) bool { return scores[i].Score > scores[j].Score })
}
