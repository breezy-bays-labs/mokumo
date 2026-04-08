<script lang="ts">
  import { page } from "$app/state";
  import { Button } from "$lib/components/ui/button";

  const STATUS_MESSAGES: Record<
    number,
    { title: string; description: string }
  > = {
    400: {
      title: "Bad request",
      description:
        "The request could not be understood. Please check the URL and try again.",
    },
    401: {
      title: "Not authorized",
      description: "You need to sign in to access this page.",
    },
    403: {
      title: "Access denied",
      description: "You do not have permission to view this page.",
    },
    404: {
      title: "Page not found",
      description:
        "The page you are looking for does not exist or has been moved.",
    },
    500: {
      title: "Something went wrong",
      description:
        "An unexpected error occurred. Please try again or return to the dashboard.",
    },
    502: {
      title: "Server unavailable",
      description:
        "The server is temporarily unreachable. Please try again in a moment.",
    },
    503: {
      title: "Service unavailable",
      description:
        "Mokumo is temporarily unavailable. Please try again shortly.",
    },
  };

  let info = $derived(
    STATUS_MESSAGES[page.status] ?? {
      title: page.status >= 500 ? "Server error" : "Something went wrong",
      description:
        page.error?.message ||
        "An unexpected error occurred. Please try again or return to the dashboard.",
    },
  );
</script>

<div class="flex min-h-screen items-center justify-center bg-background p-4">
  <div class="w-full max-w-md space-y-8 text-center">
    <div class="flex flex-col items-center gap-2">
      <img
        src="/mokumo-cloud.png"
        alt="Mokumo"
        class="h-16 dark:invert select-none"
        draggable="false"
        oncontextmenu={(e) => e.preventDefault()}
      />
      <span class="text-lg font-semibold tracking-tight">Mokumo Print</span>
    </div>

    <div class="space-y-3">
      <p class="text-7xl font-bold tracking-tighter text-muted-foreground/25">
        {page.status}
      </p>
      <h1 class="text-2xl font-bold text-foreground">
        {info.title}
      </h1>
      <p class="text-sm text-muted-foreground leading-relaxed">
        {info.description}
      </p>
    </div>

    <div class="flex flex-col items-center gap-3 pt-2">
      <Button href="/" size="lg" class="w-full max-w-xs">
        Return to Dashboard
      </Button>
      <button
        class="text-sm text-muted-foreground hover:text-foreground underline-offset-4 hover:underline transition-colors"
        onclick={() => history.back()}
      >
        Go back
      </button>
    </div>
  </div>
</div>
