<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import Description from "./description.svelte";

  const { Story } = defineMeta({
    title: "Components/Description",
    component: Description,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Description
    title="Section Title"
    description="Additional context about what this section refers to."
  />
</Story>

<Story name="Settings Context">
  <Description
    title="Shop Information"
    description="Your shop name and address appear on quotes and invoices."
  />
</Story>

<Story name="Multiple">
  <div class="flex flex-col gap-6">
    <Description
      title="General Settings"
      description="Manage your shop name, timezone, and default preferences."
    />
    <Description
      title="Team Access"
      description="Control who can access the system and their permission levels."
    />
    <Description
      title="Danger Zone"
      description="Irreversible actions that affect your entire shop."
    />
  </div>
</Story>
