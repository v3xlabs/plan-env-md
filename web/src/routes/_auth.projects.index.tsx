import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { projectsQueryOptions } from "../api/projects";
import { EMPTY, LIST, PAGE_TITLE, STATUS } from "../components/Control";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { relative } from "../time";

const ProjectsPage = () => {
  const projects = useQuery(() => projectsQueryOptions);

  return (
    <div class="space-y-6">
      <h1 class={PAGE_TITLE}>Projects</h1>
      <Suspense fallback={<p class={STATUS}>Loading projects...</p>}>
        <Show
          when={projects.data && projects.data.length > 0}
          fallback={(
            <p class={EMPTY}>
              No projects yet. A project exists once a document names one, in the
              {" "}
              <code class="font-mono">meta</code>
              {" "}
              part of a push.
            </p>
          )}
        >
          <ul class={LIST}>
            <For each={projects.data}>
              {project => (
                <li>
                  <Link
                    to="/projects/$project"
                    params={{ project: project.slug }}
                    class="flex items-center gap-4 px-5 py-3.5 hover:bg-raised"
                  >
                    <ProjectFavicon project={project.slug} class="size-8" />
                    <div class="min-w-0 flex-1">
                      <p class="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
                        {project.slug}
                      </p>
                      <p class="flex min-w-0 items-center gap-1.5 text-xs text-slate-500">
                        <span class="tabular-nums">
                          {project.document_count}
                          {project.document_count === 1 ? " document" : " documents"}
                        </span>
                        <Show when={project.aliases.length > 0}>
                          <span aria-hidden="true">-</span>
                          <span class="truncate font-mono">
                            {`aka ${project.aliases.join(", ")}`}
                          </span>
                        </Show>
                      </p>
                    </div>
                    <Show when={project.last_pushed_at}>
                      {pushedAt => (
                        <span class="shrink-0 text-xs text-slate-500">{relative(pushedAt())}</span>
                      )}
                    </Show>
                  </Link>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Suspense>
    </div>
  );
};

export const Route = createFileRoute("/_auth/projects/")({
  component: ProjectsPage,
});
