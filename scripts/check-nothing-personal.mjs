#!/usr/bin/env node
// Nothing personal is in the repository.
//
// This exists because it already happened. A city, a county, a postcode, two housemates'
// first names and a full name reached a public repository inside test fixtures, and each
// one was found by a person reading a diff rather than by anything that runs.
//
// Two of them are worth remembering. The names were in `pseudonyms.rs` — the file whose
// entire job is keeping personal values out of anything that leaves the house. And after a
// replacement of "First Last" the housemates were still present as "First", once inside a
// fixture whose line wrapping split the name across a run of whitespace, so a two-word
// search could not see it either.
//
// So the check is by shape, not by name. It does not need to know who the person is to
// notice that something looks like a home address, a real mailbox, a set of coordinates or
// a live secret. What it cannot know — their actual name, their actual city — comes from
// their own machine: ENDORA_PERSONAL_VALUES lives in the git-ignored local.mk, which is
// the whole rule in one line. Personal values are configuration, not source.
//
// CI has no local.mk, so there it runs the shape rules alone. That is the correct
// degradation: the machine that holds the real values is the machine that can check for
// them.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

// Tracked files **and files not yet added**.
//
// `git ls-files` alone reads only what git already carries, so a brand-new file is
// invisible to this check until it is staged — and the moment somebody runs the check
// before `git add`, which is the natural order, it reports all-clear on a file it never
// opened. That happened: an ADR quoting a live failure carried a real city name past a
// green check and into a merged commit.
//
// A checker that lies is worse than no checker. `--others --exclude-standard` adds
// untracked files while still honouring .gitignore, so local.mk stays out by the same rule
// that keeps it out of git.
const tracked = execFileSync(
  "git",
  ["ls-files", "--cached", "--others", "--exclude-standard"],
  { encoding: "utf8" },
)
  .split("\n")
  .filter(Boolean);

/** Mailboxes that are obviously nobody's: documentation domains and forge no-reply. */
const MAILBOX_IS_FICTIONAL =
  /@(example\.(com|org|net)|b\.com?|x\.org)$|^noreply@|\.noreply\.github\.com$/i;

/**
 * Addresses that teach something and belong to no one: loopback, "any", the public
 * resolvers, link-local metadata, and one house-shaped example for documentation.
 */
const ADDRESS_IS_AN_EXAMPLE = new Set([
  "0.0.0.0",
  "1.1.1.1",
  "8.8.8.8",
  "10.0.0.1",
  "10.0.0.5",
  "100.64.0.0",
  "127.0.0.1",
  "169.254.169.254",
  "192.168.1.10",
]);

/** Checksums, not credentials. */
const HEX_IS_EXPECTED = new Set(["Cargo.lock"]);

const rules = [
  {
    what: "a real mailbox",
    find: /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g,
    wrong: (hit) => !MAILBOX_IS_FICTIONAL.test(hit),
    say: "use an @example.com address",
  },
  {
    what: "a real network address",
    find: /\b(?:\d{1,3}\.){3}\d{1,3}\b/g,
    wrong: (hit) => !ADDRESS_IS_AN_EXAMPLE.has(hit),
    say: "use 192.168.1.10, or put the real one in local.mk",
  },
  {
    what: "coordinates",
    find: /\b(?:latitude|longitude|lat|lon|lng)["']?\s*[:=]\s*-?\d{1,3}\.\d{3,}/gi,
    wrong: () => true,
    say: "a house is findable from these — round them or drop them",
  },
  {
    what: "a phone number",
    find: /\b(?:\+1[ -]?)?\(?[2-9]\d{2}\)?[ .-]\d{3}[ .-]\d{4}\b/g,
    wrong: () => true,
    say: "use 555-0100 through 555-0199, which are reserved for fiction",
  },
  {
    what: "a live-looking secret",
    find: /\b[0-9a-f]{32,}\b/g,
    wrong: (_hit, file) => !HEX_IS_EXPECTED.has(file),
    say: "secrets belong in local.mk",
  },
];

/**
 * The person's own values, from their machine and never from the repository. Absent in CI,
 * which is the point: the file that holds them is the file git does not carry.
 */
const theirOwn = (process.env.ENDORA_PERSONAL_VALUES ?? "")
  .split(",")
  .map((value) => value.trim())
  .filter((value) => value.length > 2);

const found = [];
for (const file of tracked) {
  let text;
  try {
    text = readFileSync(file, "utf8");
  } catch {
    continue; // binary, or a symlink to somewhere else
  }
  const line = (at) => text.slice(0, at).split("\n").length;

  for (const rule of rules) {
    for (const hit of text.matchAll(rule.find)) {
      if (rule.wrong(hit[0], file)) {
        found.push({ file, line: line(hit.index), what: rule.what, hit: hit[0], say: rule.say });
      }
    }
  }
  for (const value of theirOwn) {
    const at = text.indexOf(value);
    if (at !== -1) {
      found.push({
        file,
        line: line(at),
        what: "one of your own values",
        hit: value,
        say: "it is in local.mk so that it is not in here",
      });
    }
  }
}

// The rule this whole check rests on: the file holding the real values is not carried.
if (tracked.includes("local.mk")) {
  found.push({
    file: "local.mk",
    line: 1,
    what: "the file that holds your real values",
    hit: "tracked by git",
    say: "git rm --cached local.mk — it is meant to be ignored",
  });
}

if (found.length > 0) {
  console.error(`nothing-personal check: ${found.length} to look at\n`);
  for (const f of found) {
    console.error(`  ${f.file}:${f.line}  ${f.what} — ${f.hit}`);
    console.error(`    ${f.say}`);
  }
  process.exit(1);
}

const alsoChecked = theirOwn.length > 0 ? `, and ${theirOwn.length} of your own values` : "";
console.log(`nothing-personal check: ${tracked.length} files, ${rules.length} shapes${alsoChecked}`);
