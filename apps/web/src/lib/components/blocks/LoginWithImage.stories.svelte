<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import LoginWithImage from "./login-with-image.svelte";

  const { Story } = defineMeta({
    title: "Blocks/LoginWithImage",
    component: LoginWithImage,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <LoginWithImage />
</Story>

<Story name="With Footer">
  <LoginWithImage>
    {#snippet footer()}
      Don't have an account?
      <a
        href="/setup"
        class="font-medium text-primary underline-offset-4 hover:underline"
      >
        Set up your shop
      </a>
    {/snippet}
  </LoginWithImage>
</Story>

<Story name="Custom Title">
  <LoginWithImage
    title="Welcome back"
    subtitle="Sign in to manage your print shop."
  />
</Story>

<Story name="With Submit Handler">
  <LoginWithImage
    onsubmit={(e) => {
      e.preventDefault();
      console.log("Form submitted");
    }}
  />
</Story>
