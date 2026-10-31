export const meta = {
  name: 'end-of-round',
  description: "Review a round's code for duplication, correctness and missing scenarios, verify each finding, report",
  whenToUse: "At the end of a round of the baseline experiment, or before any release after parallel work. Pass {range: '<released>..<branch>', branch: '<branch>'}.",
  phases: [
    { title: 'Review', detail: 'duplication, correctness and security, scenarios' },
    { title: 'Verify', detail: 'a skeptic tries to refute each finding' },
  ],
}

// The skill hq-round, step 4: the round's code is reviewed the same way every round.
const range = (args && args.range) || ''
const branch = (args && args.branch) || ''
if (!range || !branch) throw new Error("pass {range: '<released>..<branch>', branch: '<branch>'}")

const FINDINGS = {
  type: 'object',
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          lens: { type: 'string' },
          title: { type: 'string' },
          where: { type: 'string', description: 'file:line at the branch, every copy' },
          why: { type: 'string' },
          fix: { type: 'string', description: 'the one place it should live, or the change' },
          safety: { type: 'boolean', description: 'true when it can leak, skip a check or lose work' },
        },
        required: ['lens', 'title', 'where', 'why', 'fix', 'safety'],
      },
    },
  },
  required: ['findings'],
}

const VERDICT = {
  type: 'object',
  properties: {
    real: { type: 'boolean' },
    evidence: { type: 'string' },
  },
  required: ['real', 'evidence'],
}

const where = `Repository ~/repos/Endora-agent-collaboration. The change is \`git diff ${range}\`; read files at \`${branch}\` with \`git show ${branch}:<path>\` and \`git grep ... ${branch}\`. Read-only: change nothing.`

const LENSES = [
  {
    key: 'duplication',
    agentType: 'duplication-reviewer',
    prompt: `${where} Review the range for the same job done two ways, near-duplicate helpers, dead code, rules in the wrong layer and state kept twice.`,
  },
  {
    key: 'correctness',
    prompt: `${where} Review the range for correctness and security bugs: a check that can be skipped, a secret that can reach a log, a page or a public repository, a race on money or state, a model's output used as the enforcement boundary, work that can be lost. Only findings you can make concrete (the input or sequence that fails).`,
  },
  {
    key: 'scenarios',
    prompt: `${where} The owner's standard (docs/standards.md) wants a cucumber scenario in the owner's words for every behaviour change. List each behaviour the range changes that no \`.feature\` file (domains/*/features) covers, with the commit that changed it.`,
  },
]

phase('Review')
const verified = await pipeline(
  LENSES,
  (lens) => agent(lens.prompt, { label: `review:${lens.key}`, phase: 'Review', schema: FINDINGS, agentType: lens.agentType }),
  (found, lens) =>
    parallel(((found && found.findings) || []).map((f) => () =>
      agent(
        `${where} A reviewer claims this about the range. Try to refute it: read the code it names, and say real=false if it is wrong, already handled, or not what it says. Default to real=false if you cannot confirm it.\n\nLens: ${lens.key}\nClaim: ${f.title}\nWhere: ${f.where}\nWhy: ${f.why}`,
        { label: `verify:${lens.key}`, phase: 'Verify', schema: VERDICT, effort: 'low' },
      ).then((v) => (v && v.real ? { ...f, lens: lens.key, evidence: v.evidence } : null)),
    )),
)

const confirmed = verified.flat().filter(Boolean)
const ranked = confirmed.sort((a, b) => Number(b.safety) - Number(a.safety))
log(`${ranked.length} findings survived verification (${ranked.filter((f) => f.safety).length} touching safety)`)
return { range, branch, findings: ranked }
