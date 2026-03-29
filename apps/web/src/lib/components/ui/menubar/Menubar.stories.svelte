<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import MenubarRoot from "./menubar.svelte";
  import * as Menubar from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Menubar",
    component: MenubarRoot,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Menubar.Root>
    <Menubar.Menu>
      <Menubar.Trigger>File</Menubar.Trigger>
      <Menubar.Content>
        <Menubar.Item>New Order</Menubar.Item>
        <Menubar.Item>New Quote</Menubar.Item>
        <Menubar.Separator />
        <Menubar.Item>Import...</Menubar.Item>
        <Menubar.Item>Export...</Menubar.Item>
      </Menubar.Content>
    </Menubar.Menu>
    <Menubar.Menu>
      <Menubar.Trigger>Edit</Menubar.Trigger>
      <Menubar.Content>
        <Menubar.Item
          >Undo <Menubar.Shortcut>Ctrl+Z</Menubar.Shortcut></Menubar.Item
        >
        <Menubar.Item
          >Redo <Menubar.Shortcut>Ctrl+Y</Menubar.Shortcut></Menubar.Item
        >
        <Menubar.Separator />
        <Menubar.Item
          >Select All <Menubar.Shortcut>Ctrl+A</Menubar.Shortcut></Menubar.Item
        >
      </Menubar.Content>
    </Menubar.Menu>
    <Menubar.Menu>
      <Menubar.Trigger>View</Menubar.Trigger>
      <Menubar.Content>
        <Menubar.CheckboxItem>Show Grid</Menubar.CheckboxItem>
        <Menubar.CheckboxItem checked>Show Sidebar</Menubar.CheckboxItem>
        <Menubar.Separator />
        <Menubar.Sub>
          <Menubar.SubTrigger>Layout</Menubar.SubTrigger>
          <Menubar.SubContent>
            <Menubar.RadioGroup value="list">
              <Menubar.RadioItem value="list">List</Menubar.RadioItem>
              <Menubar.RadioItem value="board">Board</Menubar.RadioItem>
              <Menubar.RadioItem value="calendar">Calendar</Menubar.RadioItem>
            </Menubar.RadioGroup>
          </Menubar.SubContent>
        </Menubar.Sub>
      </Menubar.Content>
    </Menubar.Menu>
    <Menubar.Menu>
      <Menubar.Trigger>Help</Menubar.Trigger>
      <Menubar.Content>
        <Menubar.Item>Documentation</Menubar.Item>
        <Menubar.Item>About Mokumo</Menubar.Item>
      </Menubar.Content>
    </Menubar.Menu>
  </Menubar.Root>
</Story>

<Story name="With Shortcuts">
  <Menubar.Root>
    <Menubar.Menu>
      <Menubar.Trigger>Production</Menubar.Trigger>
      <Menubar.Content>
        <Menubar.Item
          >New Job <Menubar.Shortcut>Ctrl+N</Menubar.Shortcut></Menubar.Item
        >
        <Menubar.Item
          >Print Queue <Menubar.Shortcut>Ctrl+Q</Menubar.Shortcut></Menubar.Item
        >
        <Menubar.Separator />
        <Menubar.Item
          >Batch Print <Menubar.Shortcut>Ctrl+B</Menubar.Shortcut></Menubar.Item
        >
      </Menubar.Content>
    </Menubar.Menu>
  </Menubar.Root>
</Story>
