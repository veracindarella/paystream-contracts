import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "PayStream Dashboard",
  description: "Reference employer/employee dashboard for the PayStream stream contract.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>
        <a href="#main-content" className="skip-to-content">
          Skip to content
        </a>
        <div id="main-content" tabIndex={-1}>
          {children}
        </div>
      </body>
    </html>
  );
}