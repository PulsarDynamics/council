<script lang="ts">
	import { Marked, type Tokens } from 'marked';
	import hljs from 'highlight.js/lib/core';
	import rust from 'highlight.js/lib/languages/rust';
	import python from 'highlight.js/lib/languages/python';
	import javascript from 'highlight.js/lib/languages/javascript';
	import typescript from 'highlight.js/lib/languages/typescript';
	import json from 'highlight.js/lib/languages/json';
	import bash from 'highlight.js/lib/languages/bash';
	import xml from 'highlight.js/lib/languages/xml';
	import css from 'highlight.js/lib/languages/css';
	import yaml from 'highlight.js/lib/languages/yaml';
	import ini from 'highlight.js/lib/languages/ini';
	import markdownLang from 'highlight.js/lib/languages/markdown';
	import sql from 'highlight.js/lib/languages/sql';
	import go from 'highlight.js/lib/languages/go';
	import java from 'highlight.js/lib/languages/java';
	import c from 'highlight.js/lib/languages/c';
	import cpp from 'highlight.js/lib/languages/cpp';
	import ruby from 'highlight.js/lib/languages/ruby';
	import php from 'highlight.js/lib/languages/php';
	import diff from 'highlight.js/lib/languages/diff';
	import plaintext from 'highlight.js/lib/languages/plaintext';
	import DOMPurify from 'dompurify';
	import 'highlight.js/styles/github-dark.min.css';
	import type { Event, EventEnvelope } from '$lib/types';
	import { eventKindLabel } from '$lib/api';

	// Register the languages we care about with the core highlight.js instance.
	// We map common short aliases (js, ts, sh, …) to their full modules so that
	// model output like ```rust or ```js lights up correctly.
	hljs.registerLanguage('rust', rust);
	hljs.registerLanguage('rs', rust);
	hljs.registerLanguage('python', python);
	hljs.registerLanguage('py', python);
	hljs.registerLanguage('javascript', javascript);
	hljs.registerLanguage('js', javascript);
	hljs.registerLanguage('jsx', javascript);
	hljs.registerLanguage('typescript', typescript);
	hljs.registerLanguage('ts', typescript);
	hljs.registerLanguage('tsx', typescript);
	hljs.registerLanguage('json', json);
	hljs.registerLanguage('bash', bash);
	hljs.registerLanguage('sh', bash);
	hljs.registerLanguage('shell', bash);
	hljs.registerLanguage('html', xml);
	hljs.registerLanguage('xml', xml);
	hljs.registerLanguage('svg', xml);
	hljs.registerLanguage('css', css);
	hljs.registerLanguage('yaml', yaml);
	hljs.registerLanguage('yml', yaml);
	hljs.registerLanguage('toml', ini);
	hljs.registerLanguage('ini', ini);
	hljs.registerLanguage('markdown', markdownLang);
	hljs.registerLanguage('md', markdownLang);
	hljs.registerLanguage('sql', sql);
	hljs.registerLanguage('go', go);
	hljs.registerLanguage('golang', go);
	hljs.registerLanguage('java', java);
	hljs.registerLanguage('c', c);
	hljs.registerLanguage('cpp', cpp);
	hljs.registerLanguage('c++', cpp);
	hljs.registerLanguage('cxx', cpp);
	hljs.registerLanguage('ruby', ruby);
	hljs.registerLanguage('rb', ruby);
	hljs.registerLanguage('php', php);
	hljs.registerLanguage('diff', diff);
	hljs.registerLanguage('patch', diff);
	hljs.registerLanguage('plaintext', plaintext);
	hljs.registerLanguage('text', plaintext);
	hljs.registerLanguage('txt', plaintext);

	// Single Marked instance with a custom `code` renderer that pipes fenced
	// blocks through highlight.js. GFM + breaks so newlines in agent output
	// survive the round trip.
	const md = new Marked({
		gfm: true,
		breaks: true,
		renderer: {
			code(token: Tokens.Code): string {
				const lang = (token.lang ?? '').trim().toLowerCase();
				const code = token.text;
				if (lang && hljs.getLanguage(lang)) {
					try {
						const out = hljs.highlight(code, { language: lang, ignoreIllegals: true });
						return `<pre><code class="hljs language-${lang}">${out.value}</code></pre>`;
					} catch {
						// fall through to escaped plain text
					}
				}
				const escaped = escapeHtml(code);
				return `<pre><code>${escaped}</code></pre>`;
			}
		}
	});

	function escapeHtml(s: string): string {
		return s
			.replace(/&/g, '&amp;')
			.replace(/</g, '&lt;')
			.replace(/>/g, '&gt;')
			.replace(/"/g, '&quot;')
			.replace(/'/g, '&#39;');
	}

	// DOMPurify needs a DOM. In SSR (no `window`) we just return the marked
	// output un-sanitized; the events that drive this component arrive over a
	// WebSocket, so SSR only ever sees an empty body. In the browser we run the
	// full sanitizer with the default tag/attr allow list (which already
	// permits `class` — needed for highlight.js spans — and `href`).
	function renderMarkdown(src: string): string {
		if (!src) return '';
		const html = md.parse(src, { async: false }) as string;
		if (typeof window === 'undefined') return html;
		return DOMPurify.sanitize(html, {
			USE_PROFILES: { html: true }
		});
	}

	interface Props {
		envelope: EventEnvelope;
	}
	let { envelope }: Props = $props();

	const event = $derived(envelope.event);

	// Visual treatment per event kind.
	const palette = $derived(paletteFor(event));
	const headline = $derived(headlineFor(event));

	function paletteFor(e: Event): {
		border: string;
		bg: string;
		tag: string;
		tagText: string;
	} {
		switch (e.kind.type) {
			case 'user_message':
				return {
					border: 'border-l-zinc-500',
					bg: 'bg-zinc-800/40',
					tag: 'bg-zinc-700/60',
					tagText: 'text-zinc-200'
				};
			case 'agent_message':
				return {
					border: 'border-l-sky-400',
					bg: 'bg-sky-950/30',
					tag: 'bg-sky-500/15',
					tagText: 'text-sky-200'
				};
			case 'agent_message_delta':
				// Deltas are filtered out of the rendered list; the case
				// exists only to keep the union exhaustive.
				return {
					border: 'border-l-sky-700',
					bg: 'bg-sky-950/10',
					tag: 'bg-sky-700/15',
					tagText: 'text-sky-300/80'
				};
			case 'agent_thinking':
				return {
					border: 'border-l-sky-700',
					bg: 'bg-sky-950/10',
					tag: 'bg-sky-700/15',
					tagText: 'text-sky-300/80'
				};
			case 'tool_call':
				return {
					border: 'border-l-amber-400',
					bg: 'bg-amber-950/20',
					tag: 'bg-amber-500/15',
					tagText: 'text-amber-200'
				};
			case 'tool_result':
				return {
					border: e.kind.error ? 'border-l-rose-400' : 'border-l-emerald-400',
					bg: e.kind.error ? 'bg-rose-950/20' : 'bg-emerald-950/20',
					tag: e.kind.error ? 'bg-rose-500/15' : 'bg-emerald-500/15',
					tagText: e.kind.error ? 'text-rose-200' : 'text-emerald-200'
				};
			case 'file_change':
				return {
					border: 'border-l-violet-400',
					bg: 'bg-violet-950/20',
					tag: 'bg-violet-500/15',
					tagText: 'text-violet-200'
				};
			case 'agent_status':
				return {
					border: 'border-l-zinc-600',
					bg: 'bg-zinc-900/40',
					tag: 'bg-zinc-700/40',
					tagText: 'text-zinc-300'
				};
			case 'llm_call':
				return {
					border: 'border-l-fuchsia-400',
					bg: 'bg-fuchsia-950/20',
					tag: 'bg-fuchsia-500/15',
					tagText: 'text-fuchsia-200'
				};
			case 'system':
				return {
					border: 'border-l-zinc-700',
					bg: 'bg-transparent',
					tag: 'bg-zinc-800/40',
					tagText: 'text-zinc-400'
				};
			case 'session_created':
				return {
					border: 'border-l-emerald-500',
					bg: 'bg-emerald-950/30',
					tag: 'bg-emerald-500/15',
					tagText: 'text-emerald-200'
				};
			case 'session_completed':
				return {
					border: 'border-l-emerald-500',
					bg: 'bg-emerald-950/30',
					tag: 'bg-emerald-500/15',
					tagText: 'text-emerald-200'
				};
			case 'session_cancelled':
				return {
					border: 'border-l-amber-500',
					bg: 'bg-amber-950/30',
					tag: 'bg-amber-500/15',
					tagText: 'text-amber-200'
				};
			case 'error':
				return {
					border: 'border-l-rose-500',
					bg: 'bg-rose-950/30',
					tag: 'bg-rose-500/15',
					tagText: 'text-rose-200'
				};
		}
	}

	function headlineFor(e: Event): string {
		return eventKindLabel(e);
	}

	function bodyFor(e: Event): string {
		const k = e.kind;
		switch (k.type) {
			case 'user_message':
				return k.content;
			case 'agent_message':
				return k.content;
			case 'agent_thinking':
				return k.content;
			case 'system':
				return k.message;
			case 'session_created':
				return `Goal: ${k.goal}`;
			case 'session_completed':
				return k.summary;
			case 'session_cancelled':
				return k.reason;
			case 'error':
				return `${k.source}: ${k.message}`;
			case 'llm_call':
				return `${k.model} — ${k.prompt_tokens} in / ${k.completion_tokens} out · ${k.duration_ms}ms`;
			case 'agent_status':
				return `${k.agent} → ${k.status}`;
			default:
				return '';
		}
	}

	function extraFor(e: Event): string {
		const k = e.kind;
		if (k.type === 'tool_call') return JSON.stringify(k.args, null, 2);
		if (k.type === 'tool_result') {
			return k.error
				? `error: ${k.error}\n${JSON.stringify(k.result, null, 2)}`
				: JSON.stringify(k.result, null, 2);
		}
		if (k.type === 'file_change' && k.diff) return k.diff;
		return '';
	}

	const body = $derived(bodyFor(event));
	const extra = $derived(extraFor(event));
	const hasExtra = $derived(extra.length > 0);
	const parsedBody = $derived(renderMarkdown(body));
	const parsedExtra = $derived(hasExtra ? renderMarkdown(extra) : '');
	let expanded = $state(false);

	const time = $derived(formatTime(event.timestamp));

	function formatTime(iso: string): string {
		try {
			const d = new Date(iso);
			return d.toLocaleTimeString(undefined, {
				hour: '2-digit',
				minute: '2-digit',
				second: '2-digit',
				hour12: false
			});
		} catch {
			return iso;
		}
	}
</script>

<article
	class="border-base-300/40 rounded-md border border-l-2 p-3 font-mono text-xs {palette.bg} {palette[
		'border'
	]}"
>
	<header class="mb-1.5 flex items-center justify-between gap-2">
		<div class="flex items-center gap-2">
			<span class="rounded px-1.5 py-0.5 text-[10px] font-semibold {palette.tag} {palette.tagText}">
				{headline}
			</span>
			<span class="text-base-content/40 text-[10px]">on {envelope.channel}</span>
		</div>
		<span class="text-base-content/40 text-[10px]">{time}</span>
	</header>

	{#if parsedBody}
		<!-- Tailwind arbitrary variants style the rendered markdown without
		     pulling in the @tailwindcss/typography plugin. `not(pre_code)` keeps
		     inline code visually distinct from fenced blocks. -->
		<div
			class="markdown-body text-base-content/85 m-0 overflow-x-auto font-sans text-sm leading-relaxed [&_p]:my-1.5 [&_ul]:my-1 [&_ol]:my-1 [&_li]:my-0.5 [&_a]:underline [&_a]:text-sky-300 [&_h1]:mt-2 [&_h1]:mb-1 [&_h1]:text-base [&_h1]:font-semibold [&_h2]:mt-2 [&_h2]:mb-1 [&_h2]:text-sm [&_h2]:font-semibold [&_h3]:mt-1.5 [&_h3]:mb-1 [&_h3]:text-sm [&_h3]:font-semibold [&_blockquote]:my-1 [&_blockquote]:border-l-2 [&_blockquote]:border-base-content/30 [&_blockquote]:pl-2 [&_blockquote]:italic [&_blockquote]:text-base-content/70 [&_hr]:my-2 [&_hr]:border-base-content/20 [&_pre]:bg-base-300/30 [&_pre]:my-1.5 [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:p-2 [&_pre]:font-mono [&_pre]:text-[11px] [&_pre]:leading-relaxed [&_code:not(pre_code)]:bg-base-300/40 [&_code:not(pre_code)]:rounded [&_code:not(pre_code)]:px-1 [&_code:not(pre_code)]:py-0.5 [&_code:not(pre_code)]:font-mono [&_code:not(pre_code)]:text-[0.9em]"
		>
			{@html parsedBody}
		</div>
	{/if}

	{#if hasExtra}
		<button
			type="button"
			class="text-base-content/50 hover:text-base-content/80 mt-1 text-[10px] underline"
			onclick={() => (expanded = !expanded)}
		>
			{expanded ? 'hide details' : 'show details'}
		</button>
		{#if expanded}
			<div
				class="markdown-body text-base-content/70 bg-base-300/30 mt-1 overflow-x-auto rounded p-2 text-[11px] [&_p]:my-1 [&_pre]:bg-base-300/50 [&_pre]:my-1 [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:p-2 [&_pre]:font-mono [&_pre]:text-[11px] [&_code:not(pre_code)]:bg-base-300/50 [&_code:not(pre_code)]:rounded [&_code:not(pre_code)]:px-1 [&_code:not(pre_code)]:font-mono"
			>
				{@html parsedExtra}
			</div>
		{/if}
	{/if}
</article>
