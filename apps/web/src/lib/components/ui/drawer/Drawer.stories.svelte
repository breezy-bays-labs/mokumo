<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import * as Drawer from "./index.js";
  import DrawerRoot from "./drawer.svelte";
  import Button from "$lib/components/ui/button/button.svelte";
  import Input from "$lib/components/ui/input/input.svelte";
  import Label from "$lib/components/ui/label/label.svelte";

  const { Story } = defineMeta({
    title: "UI/Drawer",
    component: DrawerRoot,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Drawer.Root>
    <Drawer.Trigger>
      <Button variant="outline">Open Drawer</Button>
    </Drawer.Trigger>
    <Drawer.Content>
      <Drawer.Header>
        <Drawer.Title>Edit Profile</Drawer.Title>
        <Drawer.Description
          >Make changes to your profile here.</Drawer.Description
        >
      </Drawer.Header>
      <div class="px-4 py-2 space-y-4">
        <div class="space-y-2">
          <Label for="name">Name</Label>
          <Input id="name" placeholder="Shop name" />
        </div>
        <div class="space-y-2">
          <Label for="email">Email</Label>
          <Input id="email" type="email" placeholder="shop@example.com" />
        </div>
      </div>
      <Drawer.Footer>
        <Button>Save changes</Button>
        <Drawer.Close>
          <Button variant="outline">Cancel</Button>
        </Drawer.Close>
      </Drawer.Footer>
    </Drawer.Content>
  </Drawer.Root>
</Story>

<Story name="With Snap Points">
  <Drawer.Root snapPoints={[0.5, 1]}>
    <Drawer.Trigger>
      <Button variant="outline">Snap Points</Button>
    </Drawer.Trigger>
    <Drawer.Content>
      <Drawer.Header>
        <Drawer.Title>Order Details</Drawer.Title>
        <Drawer.Description
          >Drag up to see full order information.</Drawer.Description
        >
      </Drawer.Header>
      <div class="px-4 py-2">
        <p class="text-sm text-muted-foreground">
          Order #1042 — 500 custom t-shirts with 3-color screen print. Estimated
          completion: 3 business days.
        </p>
      </div>
      <Drawer.Footer>
        <Drawer.Close>
          <Button variant="outline">Close</Button>
        </Drawer.Close>
      </Drawer.Footer>
    </Drawer.Content>
  </Drawer.Root>
</Story>
