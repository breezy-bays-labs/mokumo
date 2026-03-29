<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import ThemeSwitcher from "./theme-switcher.svelte";

  const { Story } = defineMeta({
    title: "Components/ThemeSwitcher",
    component: ThemeSwitcher,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <ThemeSwitcher />
</Story>
