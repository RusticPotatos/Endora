// Only synthetic reads. This plugin is loaded exclusively by the isolated benchmark.
import { readFileSync, appendFileSync } from 'node:fs';

export default {
  id: 'endora-foundation-fixtures',
  register(api) {
    const fixture = JSON.parse(readFileSync(process.env.ENDORA_FIXTURE_FILE, 'utf8'));
    const task = fixture.cases.find(c => c.id === process.env.ENDORA_FIXTURE_CASE);
    if (!task) throw new Error('Unknown foundation fixture case');
    for (const tool of fixture.tools) {
      api.registerTool({
        ...tool,
        async execute(_id, args) {
          appendFileSync(process.env.ENDORA_FIXTURE_TRACE,
            JSON.stringify({tool: tool.name, input: args, at: Date.now()}) + '\n');
          if (tool.name === 'sports_schedule') throw new Error('Source refused this read: HTTP 403');
          let result;
          if (tool.name === 'web_search') result = JSON.stringify(task.search);
          if (tool.name === 'web_fetch') result = task.pages[args.url];
          if (result === undefined) throw new Error('Fixture source unavailable');
          return {content: [{type:'text', text:result}]};
        },
      });
    }
  },
};
