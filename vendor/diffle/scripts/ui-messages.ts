import { readdirSync, readFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseSync } from 'oxc-parser';

/** Source-only extraction. Never imports shell code, providers, DOM or a review runtime. */
export function uiMessages(): Record<string, string> {
  const catalogue: Record<string, string> = {};
  const root = fileURLToPath(new URL('../../..', import.meta.url));
  function visit(value: unknown): void {
    if (Array.isArray(value)) {
      value.forEach(visit);
      return;
    }
    if (!value || typeof value !== 'object') return;
    const node = value as Record<string, unknown>;
    const callee = node.callee as { name?: string } | undefined;
    const args = node.arguments as { type: string; value?: unknown }[] | undefined;
    if (
      node.type === 'CallExpression' &&
      callee?.name === 'uiText' &&
      args?.[0]?.type === 'Literal' &&
      typeof args[0].value === 'string'
    )
      catalogue[args[0].value] = args[0].value;
    Object.values(node).forEach(visit);
  }
  function walk(dir: string): void {
    for (const item of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, item.name);
      if (item.isDirectory()) walk(path);
      else if (/\.tsx?$/.test(item.name)) {
        const { program, errors } = parseSync(path, readFileSync(path, 'utf8'));
        if (errors.length) throw new Error(`Cannot extract ${path}: ${errors[0]?.message}`);
        visit(program);
      }
    }
  }
  walk(resolve(root, 'src'));
  walk(resolve(root, 'vendor/diffle/src/client'));
  // mdiff owns these Rust labels. locale.text uses two JSON-compatible literals.
  const native = readFileSync(resolve(root, 'src-tauri/src/attention/platform.rs'), 'utf8');
  for (const match of native.matchAll(/locale\.text\(\s*("(?:[^"\\]|\\.)*")\s*,/g)) {
    const text = JSON.parse(match[1]!) as string;
    catalogue[text] = text;
  }
  return catalogue;
}
