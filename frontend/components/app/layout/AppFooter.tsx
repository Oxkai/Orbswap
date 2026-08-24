import { color } from "@/constants";
import { STELLAR } from "@/lib/stellar/config";

export function AppFooter() {
  return (
    <footer
      className="shrink-0 flex items-center h-12 justify-between px-4 md:px-12"
    >
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: "10px",
          letterSpacing: "0.06em",
          color: color.textMuted,
        }}
      >
        {`STELLAR ${STELLAR.network} · SOROBAN`}
      </span>
      <span
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: "10px",
          letterSpacing: "0.06em",
          color: color.textMuted,
        }}
      >
        © 2026 ORBSWAP
      </span>
    </footer>
  );
}
