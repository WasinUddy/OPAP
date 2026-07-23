#!/bin/sh
set -eu

expected_oscar_revision=64c5e90a26f91fb15868bcfcccde0c1e1522ac86

if [ "$#" -ne 4 ]; then
  echo "usage: verify-compat-trees.sh OSCAR_CHECKOUT ORACLE_ADAPTER_CHECKOUT OPAP_CHECKOUT SUBJECT_ADAPTER_CHECKOUT" >&2
  exit 2
fi

oscar_checkout=$1
oracle_adapter_checkout=$2
opap_checkout=$3
subject_adapter_checkout=$4

hash_stream() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  else
    sha256sum | awk '{print $1}'
  fi
}

verify_clean_tree() {
  checkout=$1
  label=$2
  git -C "$checkout" rev-parse --is-inside-work-tree >/dev/null
  git -C "$checkout" diff --quiet
  git -C "$checkout" diff --cached --quiet
  test -z "$(git -C "$checkout" status --short --untracked-files=normal)"
  revision=$(git -C "$checkout" rev-parse --verify HEAD)
  case "$revision" in
    *[!0-9a-f]*|'')
      echo "$label revision is not lowercase hexadecimal" >&2
      exit 1
      ;;
  esac
  test "${#revision}" -eq 40
  tree_sha256=$(git -C "$checkout" archive --format=tar HEAD | hash_stream)
  printf '%s_revision=%s\n' "$label" "$revision"
  printf '%s_tree_sha256=%s\n' "$label" "$tree_sha256"
}

oscar_revision=$(git -C "$oscar_checkout" rev-parse --verify HEAD)
if [ "$oscar_revision" != "$expected_oscar_revision" ]; then
  echo "OSCAR-code revision does not match the pinned oracle" >&2
  exit 1
fi

verify_clean_tree "$oscar_checkout" oscar
verify_clean_tree "$oracle_adapter_checkout" oracle_adapter
verify_clean_tree "$opap_checkout" opap
verify_clean_tree "$subject_adapter_checkout" subject_adapter

fixtures_dir=$opap_checkout/compat/tests/fixtures
conformance_sha256=$(
  {
    printf '%s\n' 'opap-adapter-conformance-v1'
    for fixture in aggregate-digest-vector.json source-digest-vector.json waveform-digest-vector.json synthetic-oscar.json synthetic-opap.json; do
      printf '%s ' "$fixture"
      if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$fixtures_dir/$fixture" | awk '{print $1}'
      else
        sha256sum "$fixtures_dir/$fixture" | awk '{print $1}'
      fi
    done
  } | hash_stream
)

printf 'adapter_conformance_sha256=%s\n' "$conformance_sha256"
