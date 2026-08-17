// Small `cn()` helper — concatenates class names and drops falsy values.
// Mirrors the shadcn/cn convention used in Council Chamber-1's
// `src/lib/utils.ts` (twMerge + clsx) but without the extra dep —
// Svelte's class attribute already dedupes and we don't need
// Tailwind-merge semantics in this codebase yet. If we hit a real
// conflict, swap this for `tailwind-merge`.

export type ClassValue = string | number | null | false | undefined | ClassValue[];

export function cn(...inputs: ClassValue[]): string {
	const out: string[] = [];
	for (const v of inputs) {
		if (!v) continue;
		if (typeof v === 'string' || typeof v === 'number') {
			out.push(String(v));
		} else if (Array.isArray(v)) {
			const inner = cn(...v);
			if (inner) out.push(inner);
		}
	}
	return out.join(' ');
}
