import { createFileRoute, Outlet, redirect } from "@tanstack/solid-router";

import { meQueryOptions } from "../api/auth";

export const Route = createFileRoute("/_auth")({
  beforeLoad: async ({ context }) => {
    const user = await context.queryClient.ensureQueryData(meQueryOptions);

    if (!user) throw redirect({ to: "/login" });

    return { user };
  },
  component: () => <Outlet />,
});
