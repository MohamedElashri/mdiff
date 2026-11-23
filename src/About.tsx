import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { ArrowUpRight } from "lucide-react";
import { BrandMark } from "./Brand";

export function About() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);
  return (
    <section className="about-page" aria-label={uiText("About mdiff")}>
      <header className="about-brand">
        <div className="about-mark">
          <BrandMark />
        </div>
        <div>
          <h3>{uiText("mdiff")}</h3>
          <p>{uiText("Native code review")}</p>
        </div>
        {version && <span className="app-version">v{version}</span>}
      </header>
      <p className="about-intro">
        {uiText(
          "A focused place to review changes, leave feedback, and return to your code.",
        )}
      </p>
      <nav className="about-links" aria-label={uiText("Project links")}>
        <a href="https://melashri.net/soft/mdiff/">
          {uiText("Project website")}
        </a>
        <a href="https://github.com/MohamedElashri/mdiff">
          {uiText("Source code")}
        </a>
        <a href="https://github.com/MohamedElashri/mdiff/issues">
          {uiText("Report an issue")}
        </a>
        <a href="https://github.com/MohamedElashri/mdiff/releases">
          {uiText("Release notes")}
        </a>
      </nav>
      <div className="about-credits">
        <h4>{uiText("People & origins")}</h4>
        <p>
          {uiText("Made and maintained by")}{" "}
          <a href="https://github.com/MohamedElashri">
            {uiText("Mohamed Elashri")}
          </a>
          .
        </p>
        <p>
          {uiText(
            "The review engine is an independently maintained hard fork of",
          )}{" "}
          <a href="https://github.com/moritzwilksch/diffle">
            {uiText("Diffle")}
          </a>
          {uiText(", originally created by Moritz Wilksch.")}
        </p>
        <p>
          {uiText("Built with ")}
          <a href="https://tauri.app">{uiText("Tauri")}</a>,{" "}
          <a href="https://react.dev">{uiText("React")}</a>
          {uiText(", and")}{" "}
          <a href="https://github.com/pierrecomputer/pierre">
            {uiText("@pierre/diffs")}
          </a>
          .
        </p>
      </div>
      <div className="about-license">
        <div>
          <strong>{uiText("Free and open source")}</strong>
          <span>
            {uiText("GPL-3.0-only · Original Diffle code: Apache-2.0")}
          </span>
        </div>
        <a href="https://github.com/MohamedElashri/mdiff/blob/main/LICENCE.md">
          {uiText("Licence & notices")}
          <ArrowUpRight size={14} />
        </a>
      </div>
      <footer className="about-support">
        <a href="https://ko-fi.com/melashri">{uiText("Support on Ko-fi")}</a>
      </footer>
    </section>
  );
}
