<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import {
    AlertDialog,
    AlertDialogTrigger,
    AlertDialogContent,
    AlertDialogHeader,
    AlertDialogTitle,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogCancel,
    AlertDialogAction,
  } from "./index.js";
  import Button from "../button/button.svelte";

  const { Story } = defineMeta({
    title: "UI/Alert Dialog",
    component: AlertDialog,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <AlertDialog open={true}>
    <AlertDialogTrigger>
      <Button variant="outline">Delete Item</Button>
    </AlertDialogTrigger>
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle>Are you sure?</AlertDialogTitle>
        <AlertDialogDescription
          >This action cannot be undone.</AlertDialogDescription
        >
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel>Cancel</AlertDialogCancel>
        <AlertDialogAction>Continue</AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</Story>
