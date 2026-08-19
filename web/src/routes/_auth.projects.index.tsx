import { useQuery } from "@tanstack/solid-query";
import { createFileRoute, Link } from "@tanstack/solid-router";
import { For, Show, Suspense } from "solid-js";

import { projectsQueryOptions } from "../api/projects";
import { ProjectFavicon } from "../components/ProjectFavicon";
import { relative } from "../time";

const ProjectsPage = () => {
  const projects = useQuery(() => projectsQueryOptions);

  return (
    <div>
      <h1 class="mb-6 text-xl font-semibold">Projects</h1>
      <Suspense fallback={<p class="text-muted">Loading projects.</p>}>
        <Show
          when={projects.data && projects.data.length > 0}
          fallback={(
            <p class="rounded-lg border border-line bg-surface p-6 text-muted">
              No projects yet. A project exists once a document names one, either
              in the
              {" "}
              <code class="font-mono text-ink">meta</code>
              {" "}
              part of a push or by editing a document.
            </p>
          )}
        >
          <ul class="grid gap-3 sm:grid-cols-2">
            <For each={projects.data}>
              {project => (
                <li>
                  <Link
                    to="/projects/$project"
                    params={{ project: project.slug }}
                    class="flex items-center gap-3 rounded-lg border border-line bg-surface p-4 hover:border-accent"
                  >
                    <ProjectFavicon
                      project={project.slug}
                      has={project.has_favicon_light || project.has_favicon_dark}
                      class="size-8 shrink-0"
                    />
                    <div class="min-w-0 flex-1">
                      <p class="truncate font-mono text-sm font-medium text-ink">
                        {project.slug}
                      </p>
                      <p class="text-xs text-muted">
                        {project.document_count}
                        {project.document_count === 1 ? " document" : " documents"}
                        <Show when={project.last_pushed_at}>
                          {pushedAt => (
                            <>
                              {" - "}
                              {relative(pushedAt())}
                            </>
                          )}
                        </Show>
                      </p>
                    </div>
                    <Show when={!project.has_favicon_light && !project.has_favicon_dark}>
                      <span class="shrink-0 font-mono text-xs text-muted">no icon</span>
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
