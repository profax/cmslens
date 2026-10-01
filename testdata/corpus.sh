#!/bin/bash
# Downloads the markup of the domains in corpus.tsv for TestCorpus.
#
# Sites answer differently depending on where you ask from: a datacenter
# address often gets a challenge page or a refusal where a home connection gets
# the real site. Run this from wherever your scanner runs.
#
#   testdata/corpus.sh /tmp/corpus              every domain in corpus.tsv
#   testdata/corpus.sh /tmp/corpus a.com b.org  only these
#   CMSLENS_CORPUS=/tmp/corpus go test -count=1 -run TestCorpus -v .
set -euo pipefail

dir=${1:?usage: corpus.sh DIR [domain...]}
shift
mkdir -p "$dir"
if [ $# -gt 0 ]; then
	domains=("$@")
else
	mapfile -t domains < <(grep -v '^#' "$(dirname "$0")/corpus.tsv" | cut -f1)
fi

fetch() {
	curl -sS -L --max-redirs 5 -m 25 --compressed --max-filesize 5242880 \
		-A 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/120.0.0.0' \
		-H 'Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8' \
		-H "Accept-Language: ${CMSLENS_LANG:-en-US,en;q=0.9}" \
		-D "$2/$1.headers" -o "$2/$1.html" "https://$1" 2>/dev/null || true
}
export -f fetch
printf '%s\n' "${domains[@]}" | xargs -P 10 -I{} bash -c 'fetch "$1" "$2"' _ {} "$dir"
echo "fetched $(find "$dir" -name '*.html' -size +0 | wc -l) of ${#domains[@]}" >&2
