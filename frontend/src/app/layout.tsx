import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { AccessGate } from "@/components/AccessGate";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Deep Research OS",
  description:
    "Autonomous multi-agent research with verified evidence, contradiction analysis and traceable citations.",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html
      lang="en"
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col">
        <AccessGate>{children}</AccessGate>
      </body>
    </html>
  );
}
