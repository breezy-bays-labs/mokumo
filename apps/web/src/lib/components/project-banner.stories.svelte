<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import ProjectBanner from "./project-banner.svelte";

  const { Story } = defineMeta({
    title: "Components/ProjectBanner",
    component: ProjectBanner,
    tags: ["autodocs"],
  });
</script>

<script>
  import Button from "$lib/components/ui/button/button.svelte";
</script>

<Story name="Info">
  <ProjectBanner
    variant="info"
    message="A new version is available."
    dismissible
  />
</Story>

<Story name="Success">
  <ProjectBanner
    variant="success"
    message="Attack Challenge Mode is enabled for this project."
  >
    {#snippet action()}
      <Button variant="ghost" size="sm" class="h-7 text-xs">Disable</Button>
    {/snippet}
  </ProjectBanner>
</Story>

<Story name="Warning">
  <ProjectBanner
    variant="warning"
    message="This project was rolled back. Review changes before continuing."
  >
    {#snippet action()}
      <Button variant="ghost" size="sm" class="h-7 text-xs"
        >Undo Rollback</Button
      >
    {/snippet}
  </ProjectBanner>
</Story>

<Story name="Error">
  <ProjectBanner
    variant="error"
    message="Payment is overdue. Update your billing to avoid service interruption."
  >
    {#snippet action()}
      <Button variant="ghost" size="sm" class="h-7 text-xs"
        >Update Billing</Button
      >
    {/snippet}
  </ProjectBanner>
</Story>

<Story name="Dismissible">
  <ProjectBanner
    variant="info"
    message="You can dismiss this notification."
    dismissible
  />
</Story>

<Story name="All Variants">
  <div class="flex flex-col">
    <ProjectBanner variant="success" message="Success banner" />
    <ProjectBanner variant="warning" message="Warning banner" />
    <ProjectBanner variant="error" message="Error banner" />
    <ProjectBanner variant="info" message="Info banner" />
  </div>
</Story>
