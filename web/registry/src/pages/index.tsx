import PageMeta from "../components/PageMeta";
import Hero from "./landing/Hero";
import DontList from "./landing/DontList";
import HowItWorks from "./landing/HowItWorks";
import Doctrine from "./landing/Doctrine";
import RegistrySection from "./landing/RegistrySection";
import StartHere from "./landing/StartHere";
import "../styles/landing.css";

export default function Landing() {
  return (
    <>
      <PageMeta
        title="NMP — Nostr Multi-Platform"
        description="One Rust core, four platform shells. Build Nostr apps on iOS, Android, desktop, and web without inheriting the protocol bugs every other client ships."
      />
      <div class="landing">
        <Hero />
        <DontList />
        <HowItWorks />
        <Doctrine />
        <RegistrySection />
        <StartHere />
        <footer class="landing-footer">
          NMP is open source.{" "}
          <a
            href="https://github.com/pablof7z/nostr-multi-platform"
            target="_blank"
            rel="noreferrer noopener"
          >
            pablof7z/nostr-multi-platform
          </a>{" "}
          on GitHub.
        </footer>
      </div>
    </>
  );
}
