<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import EmptyState from "./empty-state.svelte";
  import Users from "@lucide/svelte/icons/users";
  import FileText from "@lucide/svelte/icons/file-text";
  import Palette from "@lucide/svelte/icons/palette";

  const { Story } = defineMeta({
    title: "UI/EmptyState",
    component: EmptyState,
    tags: ["autodocs"],
  });
</script>

<Story
  name="Default"
  args={{
    icon: Users,
    title: "No customers yet",
    subtitle:
      "This is where your customers will live. Coming in the next session.",
  }}
/>

<Story
  name="Quotes"
  args={{
    icon: FileText,
    title: "No quotes yet",
    subtitle: "Quotes will appear here once you start creating them.",
  }}
/>

<Story
  name="Artwork"
  args={{
    icon: Palette,
    title: "No artwork yet",
    subtitle: "Upload and manage your artwork files here.",
  }}
/>
