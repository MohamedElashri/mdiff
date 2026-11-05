import { describe, expect, it } from 'vitest';
import { bundledThemes } from 'shiki';
import { THEMES } from '@/client/appearance/catalogue.js';
import { DEFAULTS, normalize } from '@/client/appearance/preferences.js';
import { contrast, readable } from '@/client/appearance/palette.js';
import { reviewGeometry } from '@/client/review/geometry.js';

describe('appearance catalogue', () => {
  it('provides twenty genuine bundled themes for each mode', async () => {
    expect(THEMES.filter((t) => t.mode === 'light')).toHaveLength(20);
    expect(THEMES.filter((t) => t.mode === 'dark')).toHaveLength(20);
    expect(new Set(THEMES.map((t) => t.id)).size).toBe(40);
    for (const item of THEMES) {
      const theme = (await bundledThemes[item.id]()).default;
      expect(theme.type).toBe(item.mode);
      expect(item.bg).toMatch(/^#[a-f\d]{6}$/i);
      expect(item.fg).toMatch(/^#[a-f\d]{6}$/i);
      expect(
        contrast(item.bg, readable(item.fg, item.bg, item.mode === 'dark' ? '#ffffff' : '#000000')),
      ).toBeGreaterThanOrEqual(4.5);
    }
  });
  it('recovers safely from obsolete themes, fonts, and invalid stored sizes', () => {
    expect(
      normalize({
        light: 'dracula',
        dark: 'github-light',
        appFont: 'removed',
        codeFont: 'removed',
        appSize: NaN,
        codeSize: Infinity,
      } as never),
    ).toEqual(DEFAULTS);
    expect(normalize({ appSize: 500, codeSize: -5, codeWeight: 999 })).toMatchObject({
      appSize: 18,
      codeSize: 11,
      codeWeight: 400,
    });
  });
  it('keeps virtual row measurements in step with independently sized code', () => {
    const small = reviewGeometry(16, 11);
    const large = reviewGeometry(16, 22, 600, true);
    expect(large.itemMetrics.lineHeight).toBe(34);
    expect(large.itemMetrics.lineHeight).toBeGreaterThan(small.itemMetrics.lineHeight);
    expect(large.css).toContain('--diffs-font-size: 22px');
    expect(large.css).toContain('--diffs-line-height: 34px');
    expect(large.css).toContain('font-weight: 600');
  });
});
