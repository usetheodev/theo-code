import { FolderOpen } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Section } from "./Section";

export interface ProjectSectionProps {
  projectDir: string;
  setProjectDir: (v: string) => void;
}

export function ProjectSection({ projectDir, setProjectDir }: ProjectSectionProps) {
  return (
    <Section title="Project">
      <div className="flex gap-2">
        <input
          value={projectDir}
          onChange={(e) => setProjectDir(e.target.value)}
          placeholder="/path/to/project"
          className="flex-1 px-3 py-2 bg-surface-0 border border-border rounded-lg text-[13px] text-text-0 placeholder:text-text-3 outline-none focus:border-border-focus transition-colors"
        />
        <button
          onClick={async () => {
            const selected = await open({
              directory: true,
              multiple: false,
              title: "Select Project",
            });
            if (selected) setProjectDir(selected as string);
          }}
          className="flex items-center gap-1.5 px-3 py-2 bg-surface-2 border border-border rounded-lg text-[12px] text-text-2 hover:bg-surface-3 transition-colors shrink-0"
        >
          <FolderOpen size={14} />
          Browse
        </button>
      </div>
    </Section>
  );
}
