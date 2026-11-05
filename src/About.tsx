import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { ArrowUpRight, Code2 } from "lucide-react";
import { BrandMark } from "./Brand";

export function About() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);
  return (
    <section className="about-page" aria-label="About mdiff">
      <header className="about-brand">
        <div className="about-mark">
          <BrandMark />
        </div>
        <div>
          <h3>mdiff</h3>
          <p>Native code review</p>
        </div>
        {version && <span className="app-version">v{version}</span>}
      </header>
      <p className="about-intro">
        A focused place to review changes, leave feedback, and return to your
        code.
      </p>
      <div className="about-links">
        <a href="https://github.com/MohamedElashri/mdiff">
          <Code2 size={15} />
          Source code
        </a>
        <a href="https://github.com/MohamedElashri/mdiff/issues">
          Report an issue
          <ArrowUpRight size={14} />
        </a>
        <a href="https://github.com/MohamedElashri/mdiff/releases">
          Releases
          <ArrowUpRight size={14} />
        </a>
      </div>
      <div className="about-credits">
        <h4>People &amp; origins</h4>
        <p>
          Made and maintained by{" "}
          <a href="https://github.com/MohamedElashri">Mohamed Elashri</a>.
        </p>
        <p>
          The review engine is an independently maintained hard fork of{" "}
          <a href="https://github.com/moritzwilksch/diffle">Diffle</a>,
          originally created by Moritz Wilksch.
        </p>
        <p>
          Built with <a href="https://tauri.app">Tauri</a>,{" "}
          <a href="https://react.dev">React</a>, and{" "}
          <a href="https://github.com/pierrecomputer/pierre">@pierre/diffs</a>.
        </p>
      </div>
      <footer className="about-license">
        <div>
          <strong>Free and open source</strong>
          <span>GPL-3.0-only · Original Diffle code: Apache-2.0</span>
        </div>
        <a href="https://github.com/MohamedElashri/mdiff/blob/main/LICENCE.md">
          Licence &amp; notices
          <ArrowUpRight size={14} />
        </a>
      </footer>
    </section>
  );
}
