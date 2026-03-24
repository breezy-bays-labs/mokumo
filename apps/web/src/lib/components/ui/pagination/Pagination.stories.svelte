<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import {
    Pagination,
    PaginationContent,
    PaginationItem,
    PaginationPrevious,
    PaginationNext,
    PaginationLink,
    PaginationEllipsis,
  } from "./index.js";

  const { Story } = defineMeta({
    title: "UI/Pagination",
    component: Pagination,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <Pagination count={100} perPage={10} siblingCount={1} page={1}>
    {#snippet children({ pages })}
      <PaginationContent>
        <PaginationItem>
          <PaginationPrevious
            page={{ type: "page", value: 1 }}
            isActive={false}
          />
        </PaginationItem>
        {#each pages as page (page.key)}
          {#if page.type === "ellipsis"}
            <PaginationItem>
              <PaginationEllipsis />
            </PaginationItem>
          {:else}
            <PaginationItem>
              <PaginationLink {page} isActive={page.value === 1}>
                {page.value}
              </PaginationLink>
            </PaginationItem>
          {/if}
        {/each}
        <PaginationItem>
          <PaginationNext page={{ type: "page", value: 10 }} isActive={false} />
        </PaginationItem>
      </PaginationContent>
    {/snippet}
  </Pagination>
</Story>
