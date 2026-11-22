import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";
/** Provider text has no review-store/worker dependency. Raw HTML stays literal and images never fetch. */
export default function HostingMarkdown({ text }: { text: string }) {
  return (
    <div className="markdown hosting-markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        disallowedElements={["img"]}
        unwrapDisallowed
        components={{
          a: ({ href, children }) => {
            let safe = false;
            try {
              const url = new URL(href ?? "");
              safe =
                url.protocol === "https:" && !url.username && !url.password;
            } catch {
              /* Keep relative/unsafe links as text. */
            }
            return safe ? (
              <a
                href={href}
                onClick={(event) => {
                  event.preventDefault();
                  void openUrl(href!);
                }}
              >
                {children}
              </a>
            ) : (
              <span>{children}</span>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
