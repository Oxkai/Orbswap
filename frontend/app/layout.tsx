import type { Metadata } from "next";
import { Geist_Mono } from "next/font/google";
import localFont from "next/font/local";
import "./globals.css";
import "katex/dist/katex.min.css";
import LayoutGrid from "@/components/layout/LayoutGrid";
import { getThemeCssVariables } from "@/constants";
import { Web3ProviderShell } from "@/components/providers/Web3ProviderShell";
import { cn } from "@/lib/utils";

// KMR Apparat: the site's display + body sans, loaded locally from /public/TTF.
const apparat = localFont({
  variable: "--font-sans",
  display: "swap",
  src: [
    { path: "../public/TTF/KMR-Apparat-Light.ttf", weight: "300", style: "normal" },
    { path: "../public/TTF/KMR-Apparat-Book.ttf", weight: "350", style: "normal" },
    { path: "../public/TTF/KMR-Apparat-Regular.ttf", weight: "400", style: "normal" },
    { path: "../public/TTF/KMR-Apparat-Medium.ttf", weight: "500", style: "normal" },
    { path: "../public/TTF/KMR-Apparat-Bold.ttf", weight: "700", style: "normal" },
    { path: "../public/TTF/KMR-Apparat-Heavy.ttf", weight: "800", style: "normal" },
    { path: "../public/TTF/KMR-Apparat-Black.ttf", weight: "900", style: "normal" },
  ],
});

const geistMono = Geist_Mono({
  variable: "--font-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Orbswap · N-asset stablecoin AMM on Stellar",
  description:
    "One pool for 2 to 8 stablecoins. Concentrated liquidity that stays dense at the peg and isolates a single asset when it breaks peg. Built on the polar CCMM and CSEMM curves, live on Stellar.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      className={cn("h-full", "antialiased", geistMono.variable, "font-sans", apparat.variable)}
      data-layout-grid="hidden"
      style={getThemeCssVariables("dark")}
    >
      <body
        className="min-h-full flex flex-col"
      >
        <Web3ProviderShell>
          {children}
        </Web3ProviderShell>
        {process.env.NODE_ENV === "development" && <LayoutGrid />}
      </body>
    </html>
  );
}
