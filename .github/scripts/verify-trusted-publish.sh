#!/usr/bin/env bash
# Proves a registry's trusted-publisher entry accepts this workflow, without publishing: mints the
# job's GitHub OIDC token, exchanges it for a publish token, and discards that token.
#
#   verify-trusted-publish.sh crates <crate>   # crates.io; the token is revoked at once
#   verify-trusted-publish.sh npm <package>    # npm; the token is short-lived and simply dropped
#
# Needs `permissions: id-token: write`. Run from a publish workflow's `workflow_dispatch` path, so the
# OIDC token names the same workflow file the registry entry names.
set -euo pipefail

reg="${1:?registry: crates|npm}"
pkg="${2:?package name}"
ua="justerm-verify-trusted-publish (github.com/kihyun1998/justerm)"

case "$reg" in
  crates) aud="crates.io"; url="https://crates.io/api/v1/trusted_publishing/tokens" ;;
  npm) aud="npm:registry.npmjs.org"; url="https://registry.npmjs.org/-/npm/v1/oidc/token/exchange/package/${pkg}" ;;
  *) echo "::error::unknown registry '$reg'"; exit 2 ;;
esac

if [ -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ]; then
  echo "::error::no OIDC request URL — the job lacks 'id-token: write'"; exit 1
fi
jwt="$(curl -sS --fail-with-body -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
  "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=$aud" | jq -r .value)"
test -n "$jwt" && test "$jwt" != null || { echo "::error::GitHub issued no OIDC token"; exit 1; }
echo "::add-mask::$jwt"

# Status and body are read apart: a refusal's body is the registry's reason and is printed, a
# success's body holds the token and never is.
if [ "$reg" = crates ]; then
  resp="$(curl -sS -w '\n%{http_code}' -A "$ua" -H 'Content-Type: application/json' \
    -d "{\"jwt\":\"$jwt\"}" "$url")"
else
  resp="$(curl -sS -w '\n%{http_code}' -A "$ua" -X POST -H "Authorization: Bearer $jwt" "$url")"
fi
code="${resp##*$'\n'}"
body="${resp%$'\n'*}"
if [ "${code:0:1}" != 2 ]; then
  echo "::error::$reg refused the exchange for $pkg (HTTP $code)"
  echo "$body"
  exit 1
fi
token="$(printf '%s' "$body" | jq -r .token)"
test -n "$token" && test "$token" != null || { echo "::error::$reg answered $code but no token"; exit 1; }
echo "::add-mask::$token"

if [ "$reg" = crates ]; then
  curl -sS --fail-with-body -o /dev/null -X DELETE -A "$ua" -H "Authorization: Bearer $token" "$url"
  echo "crates.io accepted the trusted publisher for $pkg; the token was revoked."
else
  echo "npm accepted the trusted publisher for $pkg; the short-lived token was dropped unused."
fi
