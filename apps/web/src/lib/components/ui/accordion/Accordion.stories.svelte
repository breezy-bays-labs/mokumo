<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import {
    Accordion,
    AccordionItem,
    AccordionTrigger,
    AccordionContent,
  } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Accordion",
    component: Accordion,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Accordion type="single" class="w-full">
    <AccordionItem value="item-1">
      <AccordionTrigger>Is it accessible?</AccordionTrigger>
      <AccordionContent
        >Yes. It adheres to the WAI-ARIA design pattern.</AccordionContent
      >
    </AccordionItem>
    <AccordionItem value="item-2">
      <AccordionTrigger>Is it styled?</AccordionTrigger>
      <AccordionContent
        >Yes. It comes with default styles via Tailwind CSS.</AccordionContent
      >
    </AccordionItem>
    <AccordionItem value="item-3">
      <AccordionTrigger>Is it animated?</AccordionTrigger>
      <AccordionContent
        >Yes. It uses CSS animations by default.</AccordionContent
      >
    </AccordionItem>
  </Accordion>
</Story>
