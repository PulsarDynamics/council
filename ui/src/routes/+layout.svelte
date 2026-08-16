<script lang="ts">
	import './layout.css';

	let { children } = $props();
</script>

<div class="bg-base-100 text-base-content min-h-screen font-sans">
	{@render children()}
</div>
