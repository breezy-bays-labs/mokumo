<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import SplitButton from "./split-button.svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";

  const { Story } = defineMeta({
    title: "Components/SplitButton",
    component: SplitButton,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <SplitButton label="Save">
    {#snippet items()}
      <DropdownMenu.Item>Save</DropdownMenu.Item>
      <DropdownMenu.Item>Save as draft</DropdownMenu.Item>
      <DropdownMenu.Item>Save and publish</DropdownMenu.Item>
    {/snippet}
  </SplitButton>
</Story>

<Story name="Secondary">
  <SplitButton label="Export" variant="secondary">
    {#snippet items()}
      <DropdownMenu.Item>Export as PDF</DropdownMenu.Item>
      <DropdownMenu.Item>Export as CSV</DropdownMenu.Item>
      <DropdownMenu.Item>Print</DropdownMenu.Item>
    {/snippet}
  </SplitButton>
</Story>

<Story name="Outline">
  <SplitButton label="Share" variant="outline">
    {#snippet items()}
      <DropdownMenu.Item>Copy link</DropdownMenu.Item>
      <DropdownMenu.Item>Email</DropdownMenu.Item>
    {/snippet}
  </SplitButton>
</Story>

<Story name="Destructive">
  <SplitButton label="Delete" variant="destructive">
    {#snippet items()}
      <DropdownMenu.Item>Delete</DropdownMenu.Item>
      <DropdownMenu.Item>Archive</DropdownMenu.Item>
    {/snippet}
  </SplitButton>
</Story>

<Story name="Disabled">
  <SplitButton label="Save" disabled>
    {#snippet items()}
      <DropdownMenu.Item>Save</DropdownMenu.Item>
    {/snippet}
  </SplitButton>
</Story>

<Story name="WithClickHandler">
  <SplitButton
    label="Save"
    onclick={() => console.log("Primary action clicked")}
  >
    {#snippet items()}
      <DropdownMenu.Item>Save as draft</DropdownMenu.Item>
      <DropdownMenu.Item>Save and publish</DropdownMenu.Item>
    {/snippet}
  </SplitButton>
</Story>
