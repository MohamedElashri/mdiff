import type { Plugin } from "vite";

const scrollHandler = [
  "\thandleScroll = () => {",
  "\t\tif (CodeView.__STOP) return;",
  "\t\tthis.suspendScrollInteractions();",
  "\t\tthis.scrollDirty = true;",
  "\t\tthis.notifyScroll();",
  "\t\tthis.render();",
  "\t};",
].join("\n");

/**
 * CodeView 1.6.1 already patches overlapping rows in place. On Linux WebKit,
 * keep the mounted range while it still covers the viewport with room to
 * scroll. The requested window can be much shorter than the mounted range
 * once wrapped rows have been measured.
 */
export function bufferLinuxDiffScroll(code: string): string {
  if (code.split(scrollHandler).length !== 2)
    throw new Error(
      "Diffs CodeView scroll handler changed; review the Linux scroll buffer.",
    );

  return code.replace(
    scrollHandler,
    scrollHandler.replace(
      "\t\tthis.render();",
      `\t\tif (!globalThis.isTauri || !/^Linux\\b/.test(globalThis.navigator?.platform) ||
\t\t    !this.canReuseLinuxScrollRows()) this.render();`,
    ) +
      `
\tcanReuseLinuxScrollRows() {
\t\tconst { firstIndex, lastIndex, stickyTop, stickyBottom } = this.renderState;
\t\t// Use the reconciled bounds of a single, contiguous mounted file.
\t\t// The normal render path owns file transitions, relayout, jumps and rebasing.
\t\tif (firstIndex < 0 || firstIndex !== lastIndex || stickyTop < 0 || stickyBottom <= stickyTop ||
\t\t    this.heightDirty || this.layoutDirtyIndex != null || this.pendingLayoutAnchor != null ||
\t\t    this.pendingScrollTarget != null || this.scrollAnimation != null || this.shouldRebaseScroll()) return false;
\t\tconst height = this.getHeight();
\t\tif (height <= 0) return false;
\t\tconst top = this.getScrollTop() - this.getItemTopOffset();
\t\tconst bottom = top + height;
\t\tconst margin = 72;
\t\treturn (stickyTop === 0 || top >= stickyTop + margin) &&
\t\t       (stickyBottom >= this.getScrollHeight() || bottom <= stickyBottom - margin);
\t}`,
  );
}

export function diffsLinuxScroll(): Plugin {
  const name = "mdiff-diffs-linux-scroll";
  function transform(code: string, id: string) {
    if (
      !id
        .replaceAll("\\", "/")
        .endsWith("/@pierre/diffs/dist/components/CodeView.js")
    )
      return;
    return { code: bufferLinuxDiffScroll(code), map: null };
  }

  return {
    name,
    enforce: "pre",
    transform,
    // Dev prebundles the dependency before regular Vite transforms run.
    config: () => ({
      optimizeDeps: { rolldownOptions: { plugins: [{ name, transform }] } },
    }),
  };
}
