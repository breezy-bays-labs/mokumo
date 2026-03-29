<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import ErrorMessage from "./error-message.svelte";

  const { Story } = defineMeta({
    title: "Components/ErrorMessage",
    component: ErrorMessage,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <ErrorMessage message="This email address is already in use." />
</Story>

<Story name="With Label">
  <ErrorMessage
    label="Email Error: "
    message="This email address is already in use."
  />
</Story>

<Story name="Small">
  <ErrorMessage size="sm" message="Required field." />
</Story>

<Story name="Large">
  <ErrorMessage size="lg" message="The request failed. Please try again." />
</Story>

<Story name="All Sizes">
  <div class="flex flex-col gap-3">
    <ErrorMessage size="sm" message="Small error message." />
    <ErrorMessage size="md" message="Medium error message." />
    <ErrorMessage size="lg" message="Large error message." />
  </div>
</Story>
