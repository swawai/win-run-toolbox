const args = Bun.argv.slice(2);

console.log(`[demo.bun] Bun ${Bun.version}`);
console.log(`[demo.bun] command=${process.env.SWAWKIT_PROJ_CORE_COMMAND_ADDRESS}`);
console.log(`[demo.bun] commandDir=${process.env.SWAWKIT_PROJ_CORE_COMMAND_DIR}`);
console.log(`[demo.bun] cwd=${process.cwd()}`);
console.log(`[demo.bun] args=${JSON.stringify(args)}`);
