<script lang="ts">
	import { onMount } from 'svelte';
	import type { HealthResponse } from '$lib/types/HealthResponse';

	let status = $state('loading...');

	onMount(async () => {
		try {
			const res = await fetch('/api/health');
			if (!res.ok) {
				status = `API error (${res.status})`;
				return;
			}
			const data: HealthResponse = await res.json();
			status = `${data.status} — v${data.version}`;
		} catch (err) {
			console.error('[health-check]', err);
			status = 'Unable to connect to API';
		}
	});
</script>

<svelte:head>
	<title>Mokumo Print</title>
</svelte:head>

<main class="flex min-h-screen items-center justify-center">
	<div class="text-center">
		<h1 class="text-4xl font-bold tracking-tight">Mokumo Print</h1>
		<p class="mt-2 text-muted-foreground">Production management for decorated apparel</p>
		<div class="mt-6 rounded-lg border bg-card px-6 py-4">
			<p class="font-mono text-sm text-card-foreground">API: {status}</p>
		</div>
	</div>
</main>
