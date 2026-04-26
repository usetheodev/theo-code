import { ReactNode } from "react";

/** Visual section wrapper shared by every Settings sub-component. */
export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <h3 className="text-[11px] uppercase tracking-wider text-text-3 font-medium">
        {title}
      </h3>
      {children}
    </section>
  );
}
