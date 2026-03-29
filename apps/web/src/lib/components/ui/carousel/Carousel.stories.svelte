<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import CarouselRoot from "./carousel.svelte";
  import * as Carousel from "./index.js";
  import Card from "$lib/components/ui/card/card.svelte";
  import CardContent from "$lib/components/ui/card/card-content.svelte";

  const { Story } = defineMeta({
    title: "UI/Carousel",
    component: CarouselRoot,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <div class="mx-auto w-full max-w-xs">
    <Carousel.Root>
      <Carousel.Content>
        {#each Array(5) as _, i}
          <Carousel.Item>
            <Card>
              <CardContent
                class="flex aspect-square items-center justify-center p-6"
              >
                <span class="text-4xl font-semibold">{i + 1}</span>
              </CardContent>
            </Card>
          </Carousel.Item>
        {/each}
      </Carousel.Content>
      <Carousel.Previous />
      <Carousel.Next />
    </Carousel.Root>
  </div>
</Story>

<Story name="Sizes">
  <div class="mx-auto w-full max-w-sm">
    <Carousel.Root>
      <Carousel.Content class="-ml-2">
        {#each Array(6) as _, i}
          <Carousel.Item class="basis-1/3 pl-2">
            <Card>
              <CardContent
                class="flex aspect-square items-center justify-center p-4"
              >
                <span class="text-2xl font-semibold">{i + 1}</span>
              </CardContent>
            </Card>
          </Carousel.Item>
        {/each}
      </Carousel.Content>
      <Carousel.Previous />
      <Carousel.Next />
    </Carousel.Root>
  </div>
</Story>

<Story name="Vertical">
  <div class="mx-auto w-full max-w-xs">
    <Carousel.Root orientation="vertical" class="max-h-[300px]">
      <Carousel.Content class="-mt-2 h-[250px]">
        {#each Array(5) as _, i}
          <Carousel.Item class="basis-1/2 pt-2">
            <Card>
              <CardContent class="flex items-center justify-center p-6">
                <span class="text-3xl font-semibold">{i + 1}</span>
              </CardContent>
            </Card>
          </Carousel.Item>
        {/each}
      </Carousel.Content>
      <Carousel.Previous />
      <Carousel.Next />
    </Carousel.Root>
  </div>
</Story>
