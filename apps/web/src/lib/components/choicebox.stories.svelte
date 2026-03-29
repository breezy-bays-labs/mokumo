<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import Choicebox from "./choicebox.svelte";

  const { Story } = defineMeta({
    title: "Components/Choicebox",
    component: Choicebox,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <div class="w-[360px]">
    <Choicebox
      options={[
        {
          value: "trial",
          label: "Pro Trial",
          description: "Free for two weeks",
        },
        { value: "pro", label: "Pro", description: "Get started now" },
      ]}
      value="trial"
    />
  </div>
</Story>

<Story name="Multiple Selection">
  <div class="w-[360px]">
    <Choicebox
      multiple
      options={[
        {
          value: "screen",
          label: "Screen Print",
          description: "Traditional ink-on-fabric",
        },
        {
          value: "dtf",
          label: "DTF Transfer",
          description: "Direct-to-film printing",
        },
        {
          value: "embroidery",
          label: "Embroidery",
          description: "Stitched designs",
        },
      ]}
      value={["screen", "dtf"]}
    />
  </div>
</Story>

<Story name="With Disabled Option">
  <div class="w-[360px]">
    <Choicebox
      options={[
        {
          value: "basic",
          label: "Basic",
          description: "Up to 10 orders/month",
        },
        {
          value: "pro",
          label: "Professional",
          description: "Unlimited orders",
        },
        {
          value: "enterprise",
          label: "Enterprise",
          description: "Coming soon",
          disabled: true,
        },
      ]}
      value="pro"
    />
  </div>
</Story>
